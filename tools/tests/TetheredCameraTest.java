package com.infrawrench.schist;

/** Host-JVM protocol regression tests; no USB hardware or Android runtime. */
public final class TetheredCameraTest {
    public static void main(String[] args) throws Exception {
        byte[] info = {0,0,0,0,0,0,0,0,0,0,0,2,0,0,0,1,16,14,16};
        if (!TetheredCamera.supportsCapture(info)) throw new AssertionError("capture missing");
        for (int end = 0; end < info.length; end++) {
            try {
                if (TetheredCamera.supportsCapture(java.util.Arrays.copyOf(info, end)))
                    throw new AssertionError("accepted truncated DeviceInfo at " + end);
            } catch (java.io.IOException expected) { }
        }
        info[18] = 0;
        if (TetheredCamera.supportsCapture(info)) throw new AssertionError("unsupported camera accepted");
        java.util.Arrays.fill(info, 11, 15, (byte)255);
        try { TetheredCamera.supportsCapture(info); throw new AssertionError("unbounded operation array"); }
        catch (java.io.IOException expected) { }
        System.out.println("Android PTP DeviceInfo regressions passed");
    }
}
