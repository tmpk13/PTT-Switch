package pttswitch.app;

/**
 * Desktop entry point to the app's protocol code, for the repository's tests:
 *   touch HEX...   one line per value: "touched CHANGES", "released CHANGES" or "invalid"
 * An empty argument stands for an empty value.
 */
public final class ProtocolCheck {
    public static void main(String[] args) {
        if (args.length == 0 || !args[0].equals("touch")) {
            throw new IllegalArgumentException("usage: touch HEX...");
        }
        for (int i = 1; i < args.length; i++) {
            Protocol.Touch t = Protocol.touch(bytes(args[i]));
            System.out.println(t == null ? "invalid"
                    : (t.touched ? "touched " : "released ") + t.changes);
        }
    }

    private static byte[] bytes(String hex) {
        byte[] out = new byte[hex.length() / 2];
        for (int i = 0; i < out.length; i++) {
            out[i] = (byte) Integer.parseInt(hex.substring(2 * i, 2 * i + 2), 16);
        }
        return out;
    }
}
