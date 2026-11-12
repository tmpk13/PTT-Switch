package pttswitch.app;

import java.util.UUID;

/**
 * The board's BLE interface as the app reads it. The board advertises the service UUID and,
 * as manufacturer data under company ID 0xFFFF, the touch value; once connected, the touch
 * characteristic carries the same value, read or notified. Plain Java without Android classes,
 * so the desktop tests run it on the JVM and compare its UUIDs with the firmware's.
 */
public final class Protocol {
    public static final UUID SERVICE = UUID.fromString("dbc30001-a746-42c6-b521-fbfb2ba9d69e");
    public static final UUID TOUCH = UUID.fromString("dbc30002-a746-42c6-b521-fbfb2ba9d69e");
    /** Client Characteristic Configuration descriptor: writing 01 00 turns notifications on. */
    public static final UUID CCC = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb");
    /** Bluetooth SIG company ID reserved for testing, on the advertised touch value. */
    public static final int COMPANY_ID = 0xFFFF;

    private Protocol() {
    }

    /** One touch value: whether the wire is touched, and how often that has changed. */
    public static final class Touch {
        public final boolean touched;
        /**
         * Counts every change of touched since the board powered up, wrapping at 256 (0..255).
         * A jump of more than 1 between two values means presses happened in between.
         */
        public final int changes;

        Touch(boolean touched, int changes) {
            this.touched = touched;
            this.changes = changes;
        }
    }

    /**
     * Parses a touch value: byte 0 is 1 for touched and 0 for not touched, byte 1 the change
     * count. Takes the touch characteristic's value, or the manufacturer data with the company
     * ID already removed (as Android's ScanRecord hands it over). Returns null for anything
     * else, such as a board with a different firmware.
     */
    public static Touch touch(byte[] value) {
        if (value == null || value.length != 2 || (value[0] & 0xFF) > 1) {
            return null;
        }
        return new Touch(value[0] == 1, value[1] & 0xFF);
    }
}
