package com.infrawrench.schist;

import android.app.PendingIntent;
import android.content.*;
import android.hardware.usb.*;
import android.os.Build;
import android.os.SystemClock;
import java.io.*;
import java.nio.*;
import java.util.*;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import org.json.*;

/** Native USB host PTP. Never deletes objects or changes camera storage settings. */
public final class TetheredCamera {
    private static volatile boolean cancelled;
    public static void begin() { cancelled = false; }
    public static void cancel() { cancelled = true; }
    private static void check(long deadline) throws IOException {
        if (cancelled) throw new IOException("common.cancelled");
        if (SystemClock.elapsedRealtime() >= deadline) throw new IOException("tethered.timeout");
    }
    private static UsbInterface ptp(UsbDevice device) {
        for (int i = 0; i < device.getInterfaceCount(); i++) {
            UsbInterface face = device.getInterface(i);
            if (face.getInterfaceClass() == 6 && face.getInterfaceSubclass() == 1
                    && face.getInterfaceProtocol() == 1) return face;
        }
        return null;
    }
    public static String run(Context context, String operation, String port, String destination) {
        long deadline = SystemClock.elapsedRealtime() + 120000;
        try {
            UsbManager manager = (UsbManager)context.getSystemService(Context.USB_SERVICE);
            if (manager == null) throw new IOException("common.not_available");
            if (operation.equals("discover")) {
                JSONArray cameras = new JSONArray();
                for (UsbDevice device : manager.getDeviceList().values()) {
                    if (ptp(device) == null) continue;
                    String name = device.getProductName();
                    cameras.put(new JSONObject().put("model", name == null ? device.getDeviceName() : name)
                        .put("port", device.getDeviceName()));
                }
                return new JSONObject().put("cameras", cameras).toString();
            }
            UsbDevice device = manager.getDeviceList().get(port);
            if (device == null) throw new IOException("library.import.camera_disconnected");
            permission(context, manager, device, deadline);
            try (Ptp camera = new Ptp(manager, device, deadline)) {
                if (!supportsCapture(camera.exchange(0x1001, new int[0], null)))
                    throw new IOException("tethered.unsupported");
                if (operation.equals("capture")) camera.capture(new File(destination));
            }
            return "{}";
        } catch (Exception error) {
            try { return new JSONObject().put("error", error.getMessage() == null ? "tethered.no_download" : error.getMessage()).toString(); }
            catch (JSONException impossible) { throw new AssertionError(impossible); }
        }
    }
    private static void permission(Context context, UsbManager manager, UsbDevice device, long deadline) throws Exception {
        if (manager.hasPermission(device)) return;
        String action = context.getPackageName() + ".TETHERED_USB_PERMISSION";
        CountDownLatch ready = new CountDownLatch(1);
        BroadcastReceiver receiver = new BroadcastReceiver() {
            @Override public void onReceive(Context c, Intent intent) { ready.countDown(); }
        };
        if (Build.VERSION.SDK_INT >= 33) context.registerReceiver(receiver, new IntentFilter(action), Context.RECEIVER_NOT_EXPORTED);
        else context.registerReceiver(receiver, new IntentFilter(action));
        PendingIntent result = PendingIntent.getBroadcast(context, 72,
            new Intent(action).setPackage(context.getPackageName()), PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        try {
            manager.requestPermission(device, result);
            while (!ready.await(100, TimeUnit.MILLISECONDS)) check(deadline);
            check(deadline);
            if (!manager.hasPermission(device)) throw new IOException("common.cancelled");
        } finally { context.unregisterReceiver(receiver); result.cancel(); }
    }
    static boolean supportsCapture(byte[] bytes) throws IOException {
        try {
            ByteBuffer data = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
            data.position(8);
            int chars = Byte.toUnsignedInt(data.get());
            data.position(data.position() + chars * 2 + 2);
            int count = data.getInt();
            if (count < 0 || count > data.remaining() / 2) throw new IOException("tethered.no_download");
            boolean supported = false;
            for (int i = 0; i < count; i++) supported |= Short.toUnsignedInt(data.getShort()) == 0x100e;
            return supported;
        } catch (RuntimeException error) { throw new IOException("tethered.no_download", error); }
    }
    private static final class Ptp implements AutoCloseable {
        final UsbDeviceConnection connection;
        final UsbManager manager;
        final String port;
        final UsbInterface face;
        UsbEndpoint input, output;
        long deadline;
        int transaction;
        final byte[] buffer = new byte[16384];
        int offset, available;
        boolean session;
        Ptp(UsbManager manager, UsbDevice device, long deadline) throws IOException {
            this.deadline = deadline; this.manager = manager; this.port = device.getDeviceName();
            face = ptp(device);
            if (face == null) throw new IOException("tethered.unsupported");
            connection = manager.openDevice(device);
            if (connection == null) throw new IOException("library.import.camera_disconnected");
            try {
                if (!connection.claimInterface(face, false)) throw new IOException("common.not_available");
                for (int i = 0; i < face.getEndpointCount(); i++) {
                    UsbEndpoint endpoint = face.getEndpoint(i);
                    if (endpoint.getType() != UsbConstants.USB_ENDPOINT_XFER_BULK) continue;
                    if (endpoint.getDirection() == UsbConstants.USB_DIR_IN) input = endpoint;
                    else output = endpoint;
                }
                if (input == null || output == null) throw new IOException("tethered.unsupported");
                exchange(0x1002, new int[]{1}, null);
                session = true;
            } catch (IOException error) { connection.releaseInterface(face); connection.close(); throw error; }
        }
        void read(byte[] dest, int start, int length) throws IOException {
            while (length > 0) {
                check(deadline);
                if (offset == available) {
                    offset = 0;
                    available = connection.bulkTransfer(input, buffer, buffer.length, 1000);
                    if (available < 0) {
                        available = 0;
                        if (!manager.getDeviceList().containsKey(port)) throw new IOException("library.import.camera_disconnected");
                        SystemClock.sleep(20);
                        continue; // native USB timeout; allow long exposures and cancellation
                    }
                    if (available == 0) continue; // legal USB zero-length packet
                }
                int count = Math.min(length, available - offset);
                System.arraycopy(buffer, offset, dest, start, count);
                offset += count; start += count; length -= count;
            }
        }
        byte[] exchange(int operation, int[] parameters, OutputStream file) throws IOException {
            check(deadline);
            int id = transaction++;
            ByteBuffer command = ByteBuffer.allocate(12 + parameters.length * 4).order(ByteOrder.LITTLE_ENDIAN);
            command.putInt(command.capacity()).putShort((short)1).putShort((short)operation).putInt(id);
            for (int value : parameters) command.putInt(value);
            if (connection.bulkTransfer(output, command.array(), command.capacity(), 1000) != command.capacity())
                throw new IOException("library.import.camera_disconnected");
            byte[] result = new byte[0];
            boolean hadData = false;
            while (true) {
                byte[] header = new byte[12]; read(header, 0, 12);
                ByteBuffer h = ByteBuffer.wrap(header).order(ByteOrder.LITTLE_ENDIAN);
                long length = Integer.toUnsignedLong(h.getInt());
                int type = Short.toUnsignedInt(h.getShort()), code = Short.toUnsignedInt(h.getShort()), responseId = h.getInt();
                if (length < 12 || responseId != id || (type != 2 && type != 3)) throw new IOException("tethered.no_download");
                long remaining = length - 12;
                if (type == 3) {
                    if (remaining > 20) throw new IOException("tethered.no_download");
                    byte[] params = new byte[(int)remaining]; read(params, 0, params.length);
                    if (code != 0x2001) throw new IOException("PTP 0x" + Integer.toHexString(code));
                    return result;
                }
                if (hadData || code != operation) throw new IOException("tethered.no_download");
                hadData = true;
                if (file == null && remaining > 8 * 1024 * 1024) throw new IOException("tethered.no_download");
                ByteArrayOutputStream data = file == null ? new ByteArrayOutputStream() : null;
                OutputStream sink = file == null ? data : file;
                byte[] chunk = new byte[16384];
                while (remaining > 0) {
                    int count = (int)Math.min(remaining, chunk.length);
                    read(chunk, 0, count); sink.write(chunk, 0, count); remaining -= count;
                }
                if (data != null) result = data.toByteArray();
            }
        }
        Set<Integer> handles() throws IOException {
            byte[] bytes = exchange(0x1007, new int[]{-1, 0, 0}, null);
            if (bytes.length < 4) throw new IOException("tethered.no_download");
            ByteBuffer data = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
            int count = data.getInt();
            if (count < 0 || count != data.remaining() / 4 || data.remaining() % 4 != 0) throw new IOException("tethered.no_download");
            Set<Integer> result = new LinkedHashSet<>();
            while (data.hasRemaining()) result.add(data.getInt());
            return result;
        }
        void capture(File staging) throws Exception {
            Set<Integer> before = handles(), added = new LinkedHashSet<>();
            exchange(0x100e, new int[]{0,0}, null);
            long last = SystemClock.elapsedRealtime();
            while (added.isEmpty() || SystemClock.elapsedRealtime() - last < 2000) {
                check(deadline); Thread.sleep(200);
                Set<Integer> current;
                try { current = handles(); }
                catch (IOException error) { if ("PTP 0x2019".equals(error.getMessage())) continue; throw error; }
                current.removeAll(before);
                if (added.addAll(current)) last = SystemClock.elapsedRealtime();
            }
            int index = 0;
            for (int handle : added) {
                byte[] bytes = exchange(0x1008, new int[]{handle}, null);
                if (bytes.length < 53) throw new IOException("tethered.no_download");
                ByteBuffer info = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
                if (Short.toUnsignedInt(info.getShort(4)) == 0x3001) continue; // association/folder
                long size = Integer.toUnsignedLong(info.getInt(8));
                int chars = Byte.toUnsignedInt(bytes[52]);
                if (chars < 2 || 53 + chars * 2 > bytes.length) throw new IOException("tethered.no_download");
                String name = new String(bytes, 53, (chars - 1) * 2, java.nio.charset.StandardCharsets.UTF_16LE);
                int dot = name.lastIndexOf('.');
                String ext = dot < 0 ? "" : name.substring(dot + 1);
                if (!ext.matches("[A-Za-z0-9]{1,12}")) throw new IOException("tethered.no_download");
                File target = new File(staging, String.format(Locale.ROOT, "%06d.%s", index++, ext.toLowerCase(Locale.ROOT)));
                try (FileOutputStream output = new FileOutputStream(target)) {
                    exchange(0x1009, new int[]{handle}, output);
                    output.getFD().sync();
                }
                if (size == 0 || target.length() != size) throw new IOException("tethered.no_download");
            }
        }
        @Override public void close() {
            try {
                // Releasing USB ownership is unconditional, including permission,
                // protocol, timeout and cancellation failures.
                if (session && !cancelled && SystemClock.elapsedRealtime() < deadline)
                    exchange(0x1003, new int[0], null);
            } catch (IOException ignored) { }
            finally { connection.releaseInterface(face); connection.close(); }
        }
    }
}
