# Tethered capture

The desktop local gallery's **Tethered capture** button opens a remote shutter
and immediate capture review panel on Linux and macOS. This is separate from
**Import from Camera**, which copies existing photos. Cameras must support USB
remote still capture through an installed `gphoto2` / `libgphoto2` backend.
Windows, browser, iOS and Android builds do not expose this workflow.

1. Install gphoto2: on Debian/Ubuntu, `sudo apt install gphoto2`; on Fedora,
   `sudo dnf install gphoto2`; on macOS with Homebrew, `brew install gphoto2`.
   Schist must inherit a PATH containing the executable. On macOS, launching
   from the same terminal as Homebrew may be necessary. Restart after installing.
2. Connect the camera over USB, switch on its PC remote/PTP mode if required,
   and close other applications using it. A Linux desktop photo importer/GVfs
   or macOS Image Capture/Photos can hold the USB device. Release it through
   that application's normal controls; Schist never kills other applications.
3. Choose **Refresh**, then click a camera. A check mark means its backend
   advertises remote image capture. A detected camera can still fail to capture
   due to permissions, mode, focus, battery, a full card or driver limitations.
   No devices is different from a missing gphoto2 installation.
4. **Save As…** chooses an existing destination folder and a filename *prefix*;
   it does not create the chosen file. For example choose `/photos/studio/look-a`
   to save `look-a-000001.jpg`, `look-a-000002.jpg`, and so on. Schist displays the
   chosen directory and naming examples. A prefix can contain spaces and Unicode;
   it must be 1–120 UTF-8 bytes with no slash, backslash, colon, percent sign,
   control character or leading dot. Settings persist beside `library.json` in
   `tethered-session.json`. The next number free for every delivered extension is selected,
   so existing files are never replaced, including after restarting Schist.
5. Press **Capture photo**. The shutter and download run off the UI thread.
   Completed files automatically join the watched gallery. The panel displays
   the downloaded original immediately (preferring JPEG in a RAW+JPEG pair),
   without changing the gallery's prior selection or review decisions. **Edit**
   uses the normal gallery editor and Schist sidecar workflow. Unsupported
   previews say “no preview”; the original is still imported.

There is no continuous live view or camera-setting editor in this version.
Capture review uses Schist's normal RAW/JPEG preview pipeline, up to 1600 pixels.
The camera controls its exposure, focus and file format. RAW+JPEG downloads are
both preserved under one shared sequence number. This version triggers one exposure per
button press; it does not listen for the physical shutter or run interval shoots.

**Cancel** or **Close** stops an active subprocess and its process group, waits
for it to exit on the worker, and removes private incomplete downloads. Requests
time out after 120 seconds, including long exposures. A shutter that already
fired cannot be undone. A capture whose complete files have already crossed the
publication boundary finishes its gallery import even if cancellation follows.
Completion and error messages also appear in the workspace status after closing
the panel. Errors/disconnection require selecting the camera again; Refresh re-discovers
USB ports. Cancellation releases the process's USB claim, though some hardware
may need to be power-cycled after an interrupted transfer.

Every capture passes `--keep`: Schist never asks gphoto2 to delete camera files
or change the camera's capture target. **Retention depends on camera storage:**
set the camera to save to its memory card if durable camera copies are needed;
`--keep` cannot make RAM-only camera storage persistent. Downloaded files stage
in a private directory on the destination volume, and are validated as nonempty
regular files before no-replace publication using the platform’s atomic rename primitive
where supported (including modern Linux FAT/exFAT). If the operating system or
filesystem cannot provide no-replace publication, the operation fails safely.
An unusual disk failure or external file collision during multi-file publication
can leave a partial capture set: Schist imports its completed files and shows a
warning instead of deleting them. On a failed capture, private staging is removed; no camera deletion is
requested. gphoto2 user hook scripts are disabled for these requests. Original
image data and existing Schist edits are not modified.

Backend reference: the [official gphoto2 CLI manual](https://gphoto.github.io/doc/manual/ref-gphoto2-cli.html)
documents detection, capabilities, naming and capture; the maintained
[gphoto2 manual source](https://github.com/gphoto/gphoto2/blob/master/doc/gphoto2.1)
documents `--keep`. Camera support varies with the installed libgphoto2 version;
see its [remote capture support table](https://www.gphoto.org/doc/remote/).
Schist invokes the user's separately installed executable; it does not bundle
or copy gphoto2/libgphoto2 code.

Validation: `make test-tethered`, `make check-tethered`, `make check-app-web`,
and `make check-i18n`. Automated tests use an injectable backend; they do not
certify physical camera compatibility. No physical camera was available during
implementation. New labels have machine translations for a subset of shipped
locales; diagnostic/help strings and remaining labels are explicitly English
fallbacks pending translation review. Catalog validation checks structure, not
translation quality.
