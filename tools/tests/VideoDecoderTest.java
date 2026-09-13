package com.infrawrench.schist;

import java.io.File;
import java.util.ArrayList;
import java.util.Arrays;

/** Run on Android itself with app_process; exercises actual system codecs. */
public final class VideoDecoderTest {
    private static void check(boolean ok, String message) {
        if (!ok) throw new AssertionError(message);
    }
    private static ArrayList<VideoDecoder.Frame> decode(String path, long start, long end) throws Exception {
        ArrayList<VideoDecoder.Frame> frames = new ArrayList<>();
        try (VideoDecoder decoder = new VideoDecoder(path, start, end)) {
            long deadline = System.nanoTime() + 20_000_000_000L;
            while (!decoder.ended) {
                check(System.nanoTime() < deadline, "decoder stalled");
                VideoDecoder.Frame frame = decoder.step();
                if (frame != null) frames.add(frame);
            }
        }
        return frames;
    }
    private static int luma(VideoDecoder.Frame frame, int x, int y) {
        int[] l = frame.layout;
        return frame.y[(y + l[3]) * l[4] + (x + l[2]) * l[5]] & 255;
    }
    private static double sharpness(VideoDecoder.Frame frame) {
        double sum = 0, square = 0;
        int w = frame.layout[0], h = frame.layout[1];
        for (int y = 1; y < h-1; y++) for (int x = 1; x < w-1; x++) {
            int v = luma(frame,x-1,y) + luma(frame,x+1,y) + luma(frame,x,y-1) + luma(frame,x,y+1) - 4*luma(frame,x,y);
            sum += v; square += v*v;
        }
        int count = (w-2)*(h-2);
        return square/count - (sum/count)*(sum/count);
    }
    public static void main(String[] args) throws Exception {
        String sharp = new File(args[0], "sharp.mp4").getPath();
        ArrayList<VideoDecoder.Frame> frames = decode(sharp, 0, Long.MAX_VALUE);
        check(frames.size() == 8, "frame count " + frames.size());
        int best = 0;
        for (int n = 0; n < frames.size(); n++) {
            check(frames.get(n).timeUs == n * 125_000L, "wrong PTS");
            if (sharpness(frames.get(n)) > sharpness(frames.get(best))) best = n;
        }
        check(best == 3, "sharpest frame " + best);
        check(sharpness(frames.get(3)) > sharpness(frames.get(2))*10, "sharp/blur contrast");
        VideoDecoder.Frame selected = decode(sharp, 374_900, Long.MAX_VALUE).get(0);
        check(selected.timeUs == 375_000, "recapture PTS");
        check(Arrays.equals(selected.y, frames.get(3).y), "recaptured different pixels");
        check(decode(sharp, 375_900, Long.MAX_VALUE).get(0).timeUs == 500_000, "next frame repeats previous");
        check(decode(sharp, 0, 375_000).size() == 4, "bounded search");
        ArrayList<VideoDecoder.Frame> variable = decode(new File(args[0], "variable.mp4").getPath(), 0, Long.MAX_VALUE);
        long[] times = {0,125000,250000,375000,500000,750000,1000000,1250000};
        check(variable.size() == times.length, "VFR count");
        for (int n = 0; n < times.length; n++) check(variable.get(n).timeUs == times[n], "VFR timestamp");
        try (VideoDecoder rotated = new VideoDecoder(new File(args[0], "rotated.mp4").getPath(), 0, Long.MAX_VALUE)) {
            check(rotated.rotation == 90, "rotation metadata: " + rotated.rotation);
            check(rotated.durationUs == 5_000_000, "duration");
            rotated.close();
            check(rotated.step() == null, "closed decoder should stop");
        }
        boolean rejected = false;
        try (VideoDecoder bad = new VideoDecoder(new File(args[0], "missing.mp4").getPath(), 0, Long.MAX_VALUE)) { }
        catch (Exception expected) { rejected = true; }
        check(rejected, "invalid source accepted");
        System.out.println("Android video tests passed: capture, seeking, sharpness, VFR, rotation, close, invalid input");
    }
}
