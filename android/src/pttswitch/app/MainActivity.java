package pttswitch.app;

import android.app.Activity;
import android.bluetooth.BluetoothAdapter;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.PackageManager;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Bundle;
import android.provider.Settings;
import android.widget.TextView;

/**
 * One screen: the touch wire's state in a large pad, the board's name above it and the link
 * below. The link runs while the screen is visible. Tapping the pad retries whatever stopped it
 * (a missing permission, Bluetooth off, a failed link).
 */
public final class MainActivity extends Activity implements Link.Listener {
    private static final int PERMISSIONS = 1;
    private static final int ENABLE_BLUETOOTH = 2;
    private static final String BLUETOOTH_OFF = "Bluetooth is off; tap to turn it on";

    // Static nested class only: javac 21 gives an inner or anonymous class constructor a
    // nameless MethodParameters entry for the outer instance, and d8 8.2 crashes on it.
    private final BroadcastReceiver adapterState = new AdapterState(this);
    private Link link;
    private TextView board;
    private TextView pad;
    private TextView detail;
    private GradientDrawable padShape;

    private int linkState = Link.OFF;
    private String linkText;
    private Protocol.Touch touch; // latest value from the board linked now; null before one
    /** The user chose "don't ask again", so only the app's settings page can grant access. */
    private boolean deniedForGood;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        setContentView(R.layout.main);
        board = findViewById(R.id.board);
        pad = findViewById(R.id.pad);
        detail = findViewById(R.id.detail);
        padShape = (GradientDrawable) pad.getBackground().mutate();
        pad.setOnClickListener(v -> retry());
        link = new Link(this, this);
        render();
    }

    @Override
    protected void onStart() {
        super.onStart();
        registerReceiver(adapterState, new IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED));
        begin();
    }

    @Override
    protected void onStop() {
        unregisterReceiver(adapterState);
        link.stop();
        super.onStop();
    }

    /**
     * Bluetooth switched off or on while the screen is shown: a scan dies silently with it, so
     * the link is stopped and said so, and started again once Bluetooth is back.
     */
    private void adapterChanged(int state) {
        if (state == BluetoothAdapter.STATE_TURNING_OFF || state == BluetoothAdapter.STATE_OFF) {
            link.stop();
            show(BLUETOOTH_OFF);
        } else if (state == BluetoothAdapter.STATE_ON) {
            begin();
        }
    }

    /** Starts the link, or first asks for what it needs. */
    private void begin() {
        if (!hasPermissions()) {
            if (deniedForGood) {
                show("Allow Nearby devices in the app's settings, then come back");
            } else {
                show("Needs the Nearby devices permission");
                requestPermissions(Link.permissions(), PERMISSIONS);
            }
            return;
        }
        BluetoothAdapter adapter = link.adapter();
        if (adapter == null) {
            show("This phone has no Bluetooth");
            return;
        }
        if (!adapter.isEnabled()) {
            show(BLUETOOTH_OFF);
            startActivityForResult(new Intent(BluetoothAdapter.ACTION_REQUEST_ENABLE),
                    ENABLE_BLUETOOTH);
            return;
        }
        link.start();
    }

    private void retry() {
        if (linkState != Link.OFF && linkState != Link.FAILED) {
            return; // running
        }
        if (deniedForGood && !hasPermissions()) {
            startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                    Uri.fromParts("package", getPackageName(), null)));
            return;
        }
        begin();
    }

    private boolean hasPermissions() {
        for (String p : Link.permissions()) {
            if (checkSelfPermission(p) != PackageManager.PERMISSION_GRANTED) {
                return false;
            }
        }
        return true;
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] perms, int[] results) {
        if (code != PERMISSIONS) {
            return;
        }
        if (hasPermissions()) {
            begin();
            return;
        }
        // Right after a refusal, Android offers no rationale only when it will not ask again.
        deniedForGood = false;
        for (String p : perms) {
            if (checkSelfPermission(p) != PackageManager.PERMISSION_GRANTED
                    && !shouldShowRequestPermissionRationale(p)) {
                deniedForGood = true;
            }
        }
        show(deniedForGood ? "Allow Nearby devices in the app's settings; tap to open them"
                : "Needs the Nearby devices permission; tap to ask again");
    }

    @Override
    protected void onActivityResult(int code, int result, Intent data) {
        if (code == ENABLE_BLUETOOTH) {
            if (result == RESULT_OK) {
                begin();
            } else {
                show(BLUETOOTH_OFF);
            }
        }
    }

    @Override
    public void onLinkState(int state, String text) {
        linkState = state;
        linkText = text;
        if (state == Link.SCANNING || state == Link.OFF || state == Link.FAILED) {
            touch = null; // no board linked, so its wire's state is unknown
        }
        render();
    }

    @Override
    public void onTouch(Protocol.Touch t) {
        touch = t;
        render();
    }

    /** Shows a stopped link with a message in place of its state. */
    private void show(String message) {
        linkState = Link.FAILED;
        linkText = message;
        touch = null;
        render();
    }

    private void render() {
        boolean linked = linkState == Link.CONNECTING || linkState == Link.CONNECTED;
        board.setText(linked ? linkText : "");
        if (touch == null) {
            pad.setText("--");
            pad.setTextColor(getColor(R.color.faint));
            padShape.setColor(getColor(R.color.pad));
        } else if (touch.touched) {
            pad.setText("TOUCHED");
            pad.setTextColor(getColor(R.color.on_touched));
            padShape.setColor(getColor(R.color.touched));
        } else {
            pad.setText("RELEASED");
            pad.setTextColor(getColor(R.color.fg));
            padShape.setColor(getColor(R.color.pad));
        }
        String changes = touch == null ? "" : ", " + touch.changes + " changes";
        switch (linkState) {
            case Link.SCANNING:
                detail.setText("Looking for the switch");
                break;
            case Link.CONNECTING:
                detail.setText("Connecting" + changes);
                break;
            case Link.CONNECTED:
                detail.setText("Connected" + changes);
                break;
            case Link.FAILED:
                detail.setText(linkText);
                break;
            default:
                detail.setText("Stopped; tap to start");
                break;
        }
    }

    private static final class AdapterState extends BroadcastReceiver {
        private final MainActivity activity;

        AdapterState(MainActivity activity) {
            this.activity = activity;
        }

        @Override
        public void onReceive(Context context, Intent intent) {
            activity.adapterChanged(
                    intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR));
        }
    }
}
