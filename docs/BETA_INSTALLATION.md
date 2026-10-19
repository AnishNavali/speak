# Speak unsigned beta

These beta downloads do not have Apple Developer ID signing/notarization or
Windows publisher signing. macOS builds use ad-hoc signatures, which need no
developer account and do not identify a trusted publisher. App updates still
require a verified cryptographic signature.

## Install

- **macOS:** choose the Apple Silicon or Intel DMG for your Mac, open it, and copy
  Speak to Applications. Launch Speak. If macOS blocks it because the developer
  cannot be verified, open **System Settings → Privacy & Security → Open Anyway**
  for Speak and confirm. Grant Microphone and Accessibility access when setting
  up dictation.
- **Windows:** download the x64 setup EXE. If SmartScreen shows “Windows protected
  your PC,” select **More info → Run anyway** when available. Install and allow
  microphone access. Managed computers may prohibit unsigned applications.
- **Linux:** choose the x64 DEB/RPM for your distribution or make the AppImage
  executable and run it. Wayland may request shortcut and input-control access;
  Wayland support remains experimental.

## First recording

On first launch, choose **Download and set up** in **Welcome to Speak** to get the
default **Q4 + BF16 audio** model pair (about 3.83 GB). The app shows download
progress, verifies the files, and starts the model. **Use existing model files**
opens Settings for importing a pair you already have; **Later** defers setup until
the next launch. During setup, **Continue in background** closes the prompt while
the download continues. Errors offer a download resume or startup retry.

Existing installations with both configured files present skip the prompt. You
can also download/resume manually from Settings, including in older installers
without the welcome screen.

After setup finishes, focus a text field and hold **F5**, speak, then release to
insert your words. On Macs using media keys, use **Fn + F5** or configure another
shortcut. You can also hold the floating microphone or use the test notepad.

Internet is needed for initial model/runtime downloads and requested updates.
Dictation runs locally and works offline after setup.

## Updates

“Check for updates” downloads only signature-verified updates. OS installation
warnings may still apply. Downloaded models and settings are kept between
versions. Ad-hoc macOS updates can require Accessibility approval again; moving
to Developer ID signing later can also require reapproval.

The updater currently follows GitHub's latest published, non-prerelease release.
A draft is not available to the updater until published. “Unsigned beta” labels
the distribution's signing status; it is not a separate updater channel.
