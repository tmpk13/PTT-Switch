package pttswitch.app;

import android.Manifest;
import android.bluetooth.BluetoothAdapter;
import android.bluetooth.BluetoothDevice;
import android.bluetooth.BluetoothGatt;
import android.bluetooth.BluetoothGattCallback;
import android.bluetooth.BluetoothGattCharacteristic;
import android.bluetooth.BluetoothGattDescriptor;
import android.bluetooth.BluetoothGattService;
import android.bluetooth.BluetoothManager;
import android.bluetooth.BluetoothProfile;
import android.bluetooth.le.BluetoothLeScanner;
import android.bluetooth.le.ScanCallback;
import android.bluetooth.le.ScanFilter;
import android.bluetooth.le.ScanRecord;
import android.bluetooth.le.ScanResult;
import android.bluetooth.le.ScanSettings;
import android.content.Context;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.os.ParcelUuid;

import java.util.Collections;

/**
 * Finds a board and follows its touch wire. Scans for the board's service UUID, passes on the
 * touch value from the first advertisement, connects to that board and subscribes to the touch
 * characteristic, whose notifications then carry every change. When the link drops it scans
 * again. All work runs on the main thread, and the Listener is called there.
 *
 * The caller holds the permissions from permissions() and has Bluetooth on before start().
 */
final class Link {
    static final int OFF = 0;
    static final int SCANNING = 1;
    static final int CONNECTING = 2;
    static final int CONNECTED = 3;
    /** Stopped for a reason the user has to see, such as a board that is not a ptt-switch. */
    static final int FAILED = 4;

    /**
     * Android refuses a sixth scan start within 30 s (it then returns no results for a while),
     * so a board that keeps dropping the link is retried after a pause, not at once.
     */
    private static final long RESCAN_DELAY_MS = 1000;

    interface Listener {
        /**
         * The link is now in state. text is the board's name in CONNECTING and CONNECTED, the
         * reason in FAILED, and null otherwise.
         */
        void onLinkState(int state, String text);

        /** A touch value arrived, from an advertisement or a notification. */
        void onTouch(Protocol.Touch touch);
    }

    private final Context context;
    private final Listener listener;
    private final Handler main = new Handler(Looper.getMainLooper());
    // Static nested classes only: javac 21 gives an inner or anonymous class constructor a
    // nameless MethodParameters entry for the outer instance, and d8 8.2 crashes on it.
    private final ScanCallback scanCallback = new Scan(this);
    private final BluetoothGattCallback gattCallback = new Gatt(this);
    private final Runnable rescan = this::scan;
    private final BluetoothAdapter adapter;

    private boolean running;
    private boolean scanning;
    private BluetoothGatt gatt;
    private String board;

    Link(Context context, Listener listener) {
        this.context = context;
        this.listener = listener;
        BluetoothManager manager = context.getSystemService(BluetoothManager.class);
        adapter = manager == null ? null : manager.getAdapter();
    }

    /**
     * The runtime permissions a scan and a connection need: "Nearby devices" from Android 12
     * on, location before that (where Bluetooth itself is an install-time grant).
     */
    static String[] permissions() {
        if (Build.VERSION.SDK_INT >= 31) {
            return new String[] {Manifest.permission.BLUETOOTH_SCAN,
                    Manifest.permission.BLUETOOTH_CONNECT};
        }
        return new String[] {Manifest.permission.ACCESS_FINE_LOCATION};
    }

    /** Null when the phone has no Bluetooth at all. */
    BluetoothAdapter adapter() {
        return adapter;
    }

    /** Starts looking for a board; does nothing while already running. */
    void start() {
        if (running) {
            return;
        }
        running = true;
        scan();
    }

    /** Stops scanning and drops the link. */
    void stop() {
        running = false;
        main.removeCallbacks(rescan);
        stopScan();
        closeGatt();
        setState(OFF, null);
    }

    private void scan() {
        if (!running) {
            return;
        }
        BluetoothLeScanner scanner = adapter != null && adapter.isEnabled()
                ? adapter.getBluetoothLeScanner() : null;
        if (scanner == null) {
            fail("Bluetooth is off");
            return;
        }
        ScanFilter filter = new ScanFilter.Builder()
                .setServiceUuid(new ParcelUuid(Protocol.SERVICE)).build();
        ScanSettings settings = new ScanSettings.Builder()
                .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build();
        scanner.startScan(Collections.singletonList(filter), settings, scanCallback);
        scanning = true;
        board = null;
        setState(SCANNING, null);
    }

    private void stopScan() {
        if (!scanning) {
            return;
        }
        scanning = false;
        BluetoothLeScanner scanner = adapter.getBluetoothLeScanner();
        if (scanner != null) { // null once Bluetooth is off, which ends the scan anyway
            scanner.stopScan(scanCallback);
        }
    }

    private void found(ScanResult result) {
        if (!scanning) {
            return; // a result queued before stopScan()
        }
        stopScan();
        BluetoothDevice device = result.getDevice();
        ScanRecord record = result.getScanRecord();
        // The name comes in the scan response, which a result may lack; the address is the
        // board's identity in any case.
        board = record != null && record.getDeviceName() != null
                ? record.getDeviceName() : device.getAddress();
        Protocol.Touch touch = record == null ? null
                : Protocol.touch(record.getManufacturerSpecificData(Protocol.COMPANY_ID));
        if (touch != null) {
            listener.onTouch(touch);
        }
        setState(CONNECTING, board);
        gatt = device.connectGatt(context, false, gattCallback, BluetoothDevice.TRANSPORT_LE);
        if (gatt == null) {
            relink();
        }
    }

    private void connectionChanged(BluetoothGatt g, int status, int newState) {
        if (g != gatt) {
            return; // a link already closed
        }
        if (status == BluetoothGatt.GATT_SUCCESS && newState == BluetoothProfile.STATE_CONNECTED) {
            if (!g.discoverServices()) {
                relink();
            }
        } else {
            relink(); // out of range, the board reset, or the connection attempt failed
        }
    }

    private void servicesDiscovered(BluetoothGatt g, int status) {
        if (g != gatt) {
            return;
        }
        if (status != BluetoothGatt.GATT_SUCCESS) {
            relink();
            return;
        }
        BluetoothGattService service = g.getService(Protocol.SERVICE);
        BluetoothGattCharacteristic touch =
                service == null ? null : service.getCharacteristic(Protocol.TOUCH);
        BluetoothGattDescriptor ccc = touch == null ? null : touch.getDescriptor(Protocol.CCC);
        if (ccc == null) {
            fail(board + " has no touch characteristic");
            return;
        }
        // Android forwards notifications only for characteristics enabled here; the board
        // sends them only once the descriptor write turns them on.
        g.setCharacteristicNotification(touch, true);
        ccc.setValue(BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE);
        if (!g.writeDescriptor(ccc)) {
            relink();
        }
    }

    private void subscribed(BluetoothGatt g, int status) {
        if (g != gatt) {
            return;
        }
        if (status != BluetoothGatt.GATT_SUCCESS) {
            relink();
            return;
        }
        // The board answers the subscription with the current value at once.
        setState(CONNECTED, board);
    }

    private void touchChanged(BluetoothGatt g, byte[] value) {
        if (g != gatt) {
            return;
        }
        Protocol.Touch touch = Protocol.touch(value);
        if (touch == null) {
            fail(board + " sent a touch value the app does not know");
            return;
        }
        listener.onTouch(touch);
    }

    /** Drops the link and scans again after a pause. */
    private void relink() {
        closeGatt();
        if (running) {
            setState(SCANNING, null);
            main.postDelayed(rescan, RESCAN_DELAY_MS);
        }
    }

    /** Drops the link and stays stopped until the next start(). */
    private void fail(String reason) {
        running = false;
        main.removeCallbacks(rescan);
        stopScan();
        closeGatt();
        setState(FAILED, reason);
    }

    private void closeGatt() {
        if (gatt != null) {
            gatt.disconnect();
            gatt.close();
            gatt = null;
        }
    }

    private void setState(int state, String text) {
        listener.onLinkState(state, text);
    }

    /** Scan results arrive on the main thread already. */
    private static final class Scan extends ScanCallback {
        private final Link link;

        Scan(Link link) {
            this.link = link;
        }

        @Override
        public void onScanResult(int callbackType, ScanResult result) {
            link.found(result);
        }

        @Override
        public void onScanFailed(int errorCode) {
            link.scanning = false;
            link.fail("Scan failed (error " + errorCode + ")");
        }
    }

    /** GATT callbacks arrive on a Binder thread and are handed to the main thread. */
    private static final class Gatt extends BluetoothGattCallback {
        private final Link link;

        Gatt(Link link) {
            this.link = link;
        }

        @Override
        public void onConnectionStateChange(BluetoothGatt g, int status, int newState) {
            link.main.post(() -> link.connectionChanged(g, status, newState));
        }

        @Override
        public void onServicesDiscovered(BluetoothGatt g, int status) {
            link.main.post(() -> link.servicesDiscovered(g, status));
        }

        @Override
        public void onDescriptorWrite(BluetoothGatt g, BluetoothGattDescriptor d, int status) {
            link.main.post(() -> link.subscribed(g, status));
        }

        /** Android 12 and older. */
        @Override
        public void onCharacteristicChanged(BluetoothGatt g, BluetoothGattCharacteristic c) {
            changed(g, c, c.getValue());
        }

        /** Android 13 and newer call this one alone. */
        @Override
        public void onCharacteristicChanged(BluetoothGatt g, BluetoothGattCharacteristic c,
                byte[] value) {
            changed(g, c, value);
        }

        private void changed(BluetoothGatt g, BluetoothGattCharacteristic c, byte[] value) {
            if (Protocol.TOUCH.equals(c.getUuid()) && value != null) {
                byte[] copy = value.clone(); // the stack may reuse the array for the next one
                link.main.post(() -> link.touchChanged(g, copy));
            }
        }
    }
}
