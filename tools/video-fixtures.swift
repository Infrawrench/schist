// Reproducible, original test media, encoded by AVFoundation without an
// external encoder. Run `make video-fixtures` on macOS to regenerate.
import AVFoundation
import Foundation

let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
func clip(_ name: String, times: [Double], end: Double, rotated: Bool = false) throws {
    let url = directory.appendingPathComponent(name)
    if FileManager.default.fileExists(atPath: url.path) { try FileManager.default.removeItem(at: url) }
    let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
    let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
        AVVideoCodecKey: AVVideoCodecType.h264,
        AVVideoWidthKey: 64, AVVideoHeightKey: 48,
        AVVideoCompressionPropertiesKey: [
            AVVideoAverageBitRateKey: 200_000,
            AVVideoProfileLevelKey: AVVideoProfileLevelH264BaselineAutoLevel,
            AVVideoMaxKeyFrameIntervalKey: 24,
            AVVideoAllowFrameReorderingKey: false
        ]
    ])
    if rotated { input.transform = CGAffineTransform(rotationAngle: .pi / 2) }
    let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input,
        sourcePixelBufferAttributes: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: 64, kCVPixelBufferHeightKey as String: 48])
    writer.add(input)
    guard writer.startWriting() else { throw writer.error! }
    writer.startSession(atSourceTime: .zero)
    for (n, time) in times.enumerated() {
        while !input.isReadyForMoreMediaData { Thread.sleep(forTimeInterval: 0.001) }
        var pixelBuffer: CVPixelBuffer?
        CVPixelBufferPoolCreatePixelBuffer(nil, adaptor.pixelBufferPool!, &pixelBuffer)
        let buffer = pixelBuffer!
        CVPixelBufferLockBaseAddress(buffer, [])
        let base = CVPixelBufferGetBaseAddress(buffer)!.assumingMemoryBound(to: UInt8.self)
        let stride = CVPixelBufferGetBytesPerRow(buffer)
        for y in 0..<48 { for x in 0..<64 {
            var rgb: (UInt8, UInt8, UInt8)
            if rotated {
                rgb = y < 24 ? (x < 32 ? (240, 20, 20) : (20, 240, 20)) : (x < 32 ? (20, 20, 240) : (240, 240, 20))
            } else {
                // One sharp checkerboard; its neighbours are box blurred.
                var total = 0
                let radius = n == 3 ? 0 : 3
                for dy in -radius...radius { for dx in -radius...radius {
                    total += ((max(0, x + dx) / 4 + max(0, y + dy) / 4) % 2 == 0 ? 0 : 255)
                }}
                let v = UInt8(total / ((2 * radius + 1) * (2 * radius + 1)))
                rgb = (v, v, v)
            }
            let p = base.advanced(by: y * stride + x * 4)
            p[0] = rgb.2; p[1] = rgb.1; p[2] = rgb.0; p[3] = 255
        }}
        CVPixelBufferUnlockBaseAddress(buffer, [])
        guard adaptor.append(buffer, withPresentationTime: CMTime(seconds: time, preferredTimescale: 600)) else { throw writer.error! }
    }
    writer.endSession(atSourceTime: CMTime(seconds: end, preferredTimescale: 600))
    input.markAsFinished()
    let done = DispatchSemaphore(value: 0)
    writer.finishWriting { done.signal() }
    done.wait()
    guard writer.status == .completed else { throw writer.error! }
    print(url.lastPathComponent)
}
try clip("sharp.mp4", times: (0..<8).map { Double($0) / 8 }, end: 1)
try clip("variable.mp4", times: [0, 0.125, 0.25, 0.375, 0.5, 0.75, 1, 1.25], end: 1.5)
try clip("rotated.mp4", times: (0..<120).map { Double($0) / 24 }, end: 5, rotated: true)
