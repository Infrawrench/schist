package com.infrawrench.schist;

import android.graphics.ImageFormat;
import android.graphics.Rect;
import android.media.Image;
import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaExtractor;
import android.media.MediaFormat;
import java.io.IOException;
import java.nio.ByteBuffer;

/** System decoding only. Each step releases its output buffer before returning. */
public final class VideoDecoder implements AutoCloseable {
    private MediaExtractor extractor;
    private MediaCodec codec;
    private boolean started, inputEnded;
    public boolean ended;
    public long durationUs;
    public int rotation, aspectWidth, aspectHeight;
    private final long startUs, endUs;
    private final MediaCodec.BufferInfo info = new MediaCodec.BufferInfo();

    public VideoDecoder(String path, long start, long end) throws IOException {
        startUs = Math.max(0, start);
        endUs = end;
        try {
            extractor = new MediaExtractor();
            extractor.setDataSource(path);
            MediaFormat format = null;
            for (int i = 0; i < extractor.getTrackCount(); i++) {
                MediaFormat candidate = extractor.getTrackFormat(i);
                String mime = candidate.getString(MediaFormat.KEY_MIME);
                if (mime != null && mime.startsWith("video/")) {
                    extractor.selectTrack(i);
                    format = candidate;
                    break;
                }
            }
            if (format == null) throw new IOException("video.no_stream");
            durationUs = format.getLong(MediaFormat.KEY_DURATION, 0);
            if (durationUs <= 0) throw new IOException("video.no_duration");
            rotation = format.getInteger(MediaFormat.KEY_ROTATION, 0);
            aspectWidth = format.getInteger("sar-width", 1);
            aspectHeight = format.getInteger("sar-height", 1);
            format.setInteger(MediaFormat.KEY_ROTATION, 0); // Rust applies display orientation.
            format.setInteger(MediaFormat.KEY_COLOR_FORMAT,
                    MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible);
            codec = MediaCodec.createDecoderByType(format.getString(MediaFormat.KEY_MIME));
            codec.configure(format, null, null, 0);
            codec.start();
            started = true;
            if (startUs > 0) extractor.seekTo(startUs, MediaExtractor.SEEK_TO_PREVIOUS_SYNC);
        } catch (IOException | RuntimeException error) {
            close();
            throw error;
        }
    }

    /** One bounded step lets the Rust worker check cancellation between polls. */
    public Frame step() {
        if (ended) return null;
        if (!inputEnded) {
            int index = codec.dequeueInputBuffer(0);
            if (index >= 0) {
                ByteBuffer input = codec.getInputBuffer(index);
                if (input == null) throw new IllegalStateException("video.decode_failed");
                input.clear();
                int size = extractor.readSampleData(input, 0);
                if (size < 0) {
                    codec.queueInputBuffer(index, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM);
                    inputEnded = true;
                } else {
                    if ((extractor.getSampleFlags() & MediaExtractor.SAMPLE_FLAG_ENCRYPTED) != 0)
                        throw new IllegalStateException("video.decode_failed");
                    codec.queueInputBuffer(index, 0, size, extractor.getSampleTime(), 0);
                    extractor.advance();
                }
            }
        }
        int index = codec.dequeueOutputBuffer(info, 10_000);
        if (index < 0) return null; // try again / format changed / buffers changed
        try {
            if ((info.flags & MediaCodec.BUFFER_FLAG_END_OF_STREAM) != 0) ended = true;
            if (info.size == 0 || (info.flags & MediaCodec.BUFFER_FLAG_CODEC_CONFIG) != 0 || info.presentationTimeUs + 1 < startUs) return null;
            if (info.presentationTimeUs > endUs) { ended = true; return null; }
            try (Image image = codec.getOutputImage(index)) {
                if (image == null || image.getFormat() != ImageFormat.YUV_420_888)
                    throw new IllegalStateException("video.decode_failed");
                return new Frame(image, info.presentationTimeUs, codec.getOutputFormat(index));
            }
        } finally {
            codec.releaseOutputBuffer(index, false);
        }
    }

    /** Copy planes while owned by the codec; respect row and chroma pixel strides. */
    public static final class Frame {
        public final long timeUs;
        public final int[] layout;
        public final byte[] y, u, v;
        Frame(Image image, long time, MediaFormat format) {
            timeUs = time;
            Rect crop = image.getCropRect();
            long count = (long)crop.width() * crop.height();
            if (count <= 0 || count > 64L * 1024 * 1024)
                throw new IllegalArgumentException("video.frame_too_large");
            Image.Plane[] planes = image.getPlanes();
            layout = new int[] {crop.width(), crop.height(), crop.left, crop.top,
                planes[0].getRowStride(), planes[0].getPixelStride(),
                planes[1].getRowStride(), planes[1].getPixelStride(),
                planes[2].getRowStride(), planes[2].getPixelStride(),
                format.getInteger(MediaFormat.KEY_COLOR_STANDARD,
                    crop.height() > 576 ? MediaFormat.COLOR_STANDARD_BT709 : MediaFormat.COLOR_STANDARD_BT601_NTSC),
                format.getInteger(MediaFormat.KEY_COLOR_RANGE, MediaFormat.COLOR_RANGE_LIMITED)};
            y = copy(planes[0]); u = copy(planes[1]); v = copy(planes[2]);
        }
        private static byte[] copy(Image.Plane plane) {
            ByteBuffer buffer = plane.getBuffer().duplicate();
            // Reject implausible allocations before allocating a Java array.
            if (buffer.remaining() > 128 * 1024 * 1024)
                throw new IllegalArgumentException("video.frame_too_large");
            byte[] bytes = new byte[buffer.remaining()];
            buffer.get(bytes);
            return bytes;
        }
    }

    @Override public void close() {
        if (codec != null) {
            try { if (started) codec.stop(); } catch (RuntimeException ignored) { }
            try { codec.release(); } catch (RuntimeException ignored) { } finally { codec = null; }
        }
        if (extractor != null) { extractor.release(); extractor = null; }
        ended = true;
    }
}
