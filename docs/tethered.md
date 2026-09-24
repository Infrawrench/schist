# Tethered capture

**Tethered capture** opens a remote shutter and capture review workflow. It is
separate from **Import from Camera**, which copies existing photos. No backend
launches `gphoto2`, PowerShell, or a vendor command-line program.

On macOS, the picker includes both photo cameras through ImageCaptureCore and
built-in iSight/FaceTime cameras, USB webcams and available Continuity Cameras
through AVFoundation. Webcams can be used to test the same Save As, capture,
preview and gallery import workflow without an external photo camera.

| Platform | Camera API | Requirements |
| --- | --- | --- |
| Linux | Linked libgphoto2 | Distribution runtime and USB permissions; AppImage bundles camera and port modules. |
| macOS | ImageCaptureCore and AVFoundation | A photo camera advertising remote picture capture, or a webcam with camera permission. No additional software. |
| Windows | Windows Portable Devices (WPD) | The camera's installed PTP/MTP driver must advertise still-image capture. No replacement USB driver required. |
| iOS/iPadOS | ImageCaptureCore PTP commands | iOS 15.2 or later, camera permission, compatible USB adapter, standard PTP InitiateCapture support. |
| Android | Android USB host API and PTP | USB host/OTG support, per-device USB permission, standard PTP InitiateCapture support. |
| Browser | WebUSB and PTP | HTTPS/localhost, a WebUSB-capable browser, camera permission, an interface the OS permits the browser to claim. |

Vendor-specific remote modes are handled by libgphoto2 on Linux. The standard PTP
backends cannot operate cameras that require proprietary capture commands. WPD
support depends on the Windows driver. Merely appearing in a device picker does
not establish shutter support; the backend checks capabilities before capture.
Browsers without WebUSB report that capture is unavailable. OS-owned interfaces
may be unavailable to WebUSB even when the native app can use the camera.

1. On native Linux packages, install the libgphoto2 runtime supplied by the
   distribution. Source builds also need its development headers and
   `pkg-config`; on Debian/Ubuntu that package is `libgphoto2-dev`. The macOS
   workflow needs no extra camera software. The AppImage carries the Linux
   library, port library and camera modules it needs.
2. Connect the camera over USB, switch on its PC remote/PTP mode if required,
   and close other applications using it. A desktop photo importer, GVfs,
   Image Capture or Photos can hold the USB device. Release it through that
   application's normal controls; Schist never terminates another process.
3. Choose **Refresh**, then select a camera from the dropdown beside it. The
   dropdown displays the selected camera. A detected camera can still fail
   because of permissions, mode, focus, battery, a full card or driver
   limitations.
4. On the first **Capture photo**, choose an existing destination folder and a
   filename *prefix*. **Save As…** changes this choice later. Choosing the prefix
   does not create the chosen file. For example choose `/photos/studio/look-a`
   to save `look-a-000001.jpg`, `look-a-000002.jpg`, and so on. Schist displays
   the chosen directory and naming examples. A prefix can contain spaces and Unicode;
   it must be 1–120 UTF-8 bytes with no slash, backslash, colon, percent sign,
   control character, Windows-reserved punctuation (`<>"|?*`) or leading dot. Settings persist beside `library.json` in
   `tethered-session.json`. The next number free for every delivered extension is selected,
   so existing files are never replaced, including after restarting Schist.
5. The first capture starts after you choose the destination. Press **Capture
   photo** again for each subsequent shot. Completed files automatically join the watched
   gallery. The panel displays the downloaded original immediately (preferring
   JPEG in a RAW+JPEG pair), without changing the gallery's prior selection or
   review decisions. **Edit** uses the normal gallery editor and Schist sidecar
   workflow. If a preview cannot be decoded, the panel explains that the original
   was saved and imported.

There is no continuous live view or camera-setting editor in this version.
Capture review uses Schist's normal RAW/JPEG preview pipeline, up to 1600 pixels.
Photo cameras control their exposure, focus and file format. RAW+JPEG downloads are
preserved under one shared sequence number when delivered by the driver. The
backend waits for two seconds without new files to collect companion files;
cameras that deliver companions later require hardware-specific validation. This version triggers one
exposure per button press; it does not listen for the physical shutter or run
interval shoots.

For macOS webcams, **Capture photo** requests camera permission if needed, opens
the camera briefly, allows automatic exposure to settle, and saves one JPEG
snapshot from its video stream at the native high-quality session preset. It
does not record audio, provide RAW output or retain a copy on the camera. The
camera is released after each capture, error or cancellation. Webcam capture
has a 30-second deadline after permission is granted; the permission prompt can
wait up to 120 seconds. Native startup/shutdown may take longer to return.

If camera permission is denied, allow it in **System Settings → Privacy &
Security → Camera**. For terminal launches, macOS may associate permission with
the terminal application. `make app PROFILE=debug` embeds the required privacy
metadata in `target/debug/schist`, so a source build also works outside an app
bundle. The signed app includes the camera entitlement. Discovery alone neither
requests permission nor starts a camera.

**Cancel** or **Close** asks the active backend to stop, releases its camera
session, and removes private incomplete downloads. The capture deadline is
120 seconds, including long exposures. Native drivers may take longer to return
from a blocking call or acknowledge cancellation; Schist keeps the session and
staging alive until native I/O has stopped. A shutter that already fired cannot
be undone. A capture whose complete files have crossed the publication boundary
finishes its gallery import even if cancellation follows. Completion and error
messages also appear in the workspace status after closing the panel. An error
or disconnection requires selecting the camera again; Refresh rediscovers
connected devices.

Schist never requests deletion from the camera and does not change its capture
target or storage settings. **Retention depends on camera storage:** save to the
memory card when durable camera copies are needed; RAM-only captures disappear
when the camera disconnects. Downloaded files stage in a private directory on
the destination volume and are validated as nonempty regular files before
atomic no-replace publication. If the
filesystem cannot provide no-replace publication, the operation fails safely.
An unusual disk failure or external collision during multi-file publication can
leave a partial capture set: Schist imports its completed files and shows a
warning instead of deleting them. Failed or cancelled downloads are removed from
private staging. Original image data and existing Schist edits are not modified.

On mobile, **Save As…** uses Schist's folder/name picker so selecting a session
does not export an empty placeholder file. Choose a directory writable by Schist.
Android asks for USB permission when connecting. iOS asks for camera access when
the ImageCaptureCore session opens.

In the browser, **Refresh** opens the browser's USB permission picker. **Capture
photo** retains complete originals in the tab and lists them with **Save As…**
(download) and **Edit** actions. Download each original before closing/reloading
the tab: browser captures are not a watched disk gallery and are not uploaded to
Schist Cloud. Browser downloads use the browser's filename/collision policy.
A capture set larger than 512 MiB is rejected before allocation. Native sessions
continue to use the destination directory, durable publication and gallery preview
workflow described above.

API references: [Apple ImageCaptureCore](https://developer.apple.com/documentation/imagecapturecore),
[Apple AVFoundation capture](https://developer.apple.com/documentation/avfoundation/capture-setup),
[libgphoto2](https://www.gphoto.org/doc/devapi/),
[Windows still-image capture](https://learn.microsoft.com/en-us/windows/win32/wpd_sdk/wpd-command-still-image-capture-initiate-command),
[Android USB host](https://developer.android.com/develop/connectivity/usb/host), and
[WebUSB](https://developer.chrome.com/docs/capabilities/usb).

Validation targets: `make test-tethered`, `make test-tethered-editor`,
`make test-tethered-web`, `make test-tethered-android`, `make check-tethered`,
`make check-tethered-backend TETHERED_TARGET=aarch64-pc-windows-msvc`,
`make check-camera-sync-ios`, `make check-camera-sync-android`,
`make check-app-web`, and `make check-i18n`. Tests cover publication, configuration,
PTP parsing, bounded transfers, cancellation, and Objective-C delegate signatures.
They do not certify physical camera compatibility. Physical PTP-camera captures
were not exercised during implementation. Test Windows device behavior on Windows;
cross-compilation only checks API/type correctness.

On macOS, `make test-tethered-webcam-discovery` checks webcam enumeration without
opening a camera. `make test-tethered-webcam` is an opt-in hardware test that may
prompt for camera access: it captures temporary JPEGs, decodes one, checks
cancellation and reopening, and verifies that existing captures are not replaced.
Temporary captures are removed when the test finishes.

The tethered-capture catalog has AI-assisted translations in all 149 non-English
shipped locales. Native-speaker review is still pending. Catalog validation
checks structure, placeholders and font coverage, not translation quality.
