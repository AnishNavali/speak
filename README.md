<img src="docs/media/logo.png" alt="Speak logo" width="80" height="80">

# Speak

**Hold a key. Speak naturally. Your words appear where you're typing.**

Speak turns your voice into text and tidies up the result, right on your computer.
Use it for messages, emails, notes, and other everyday writing. Your recordings
are processed locally, without sending them to a cloud transcription service.

**[Download](#download-speak) · [Watch the demo](#see-speak-in-action) · [Get started](#get-started) · [Need help?](#need-help)**

## See Speak in action

Hold the shortcut, speak, and release to turn your words into text.

https://github.com/user-attachments/assets/11d62b96-b051-4f86-8d68-9d49f6dbcde5

## Download Speak

> **The first public release is being prepared.** The direct download links below
> will work once it is published. Check [all releases](https://github.com/AnishNavali/speak/releases)
> for availability. Passing build checks does not publish the installers.

Choose your computer below. You don't need programming tools or terminal commands
to install the Mac or Windows app.

| Your computer                       | Download                                                                                                                           |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Mac with an Apple chip, M1 or newer | **[Download for Mac — Apple Silicon](https://github.com/AnishNavali/speak/releases/latest/download/Speak-macOS-AppleSilicon.dmg)** |
| Mac with an Intel processor         | **[Download for Mac — Intel](https://github.com/AnishNavali/speak/releases/latest/download/Speak-macOS-Intel.dmg)**                |
| Windows PC, 64-bit Intel or AMD     | **[Download for Windows](https://github.com/AnishNavali/speak/releases/latest/download/Speak-Windows-x64-Setup.exe)**              |
| Linux PC, 64-bit Intel or AMD       | **[Download for Linux — AppImage](https://github.com/AnishNavali/speak/releases/latest/download/Speak-Linux-x64.AppImage)**        |

Linux packages: [DEB](https://github.com/AnishNavali/speak/releases/latest/download/Speak-Linux-x64.deb)
· [RPM](https://github.com/AnishNavali/speak/releases/latest/download/Speak-Linux-x64.rpm).
Linux builds use Ubuntu 24.04 as their baseline; compatibility with other
distributions can vary. Wayland support is experimental.

Not sure which Mac you have? Open **Apple menu → About This Mac**. If it says
**Chip: Apple M…**, choose Apple Silicon. If it says **Processor: Intel…**, choose Intel.
Speak requires macOS **13.3 or later**.

The installers are in this repository's **Releases** section. The green **Code**
button and **Source code** ZIP files are for developers, not for installing Speak.

## Get started

### 1. Install and open Speak

- **Mac:** open the downloaded `.dmg` file, drag **Speak** to **Applications**, then
  open Speak from Applications.
- **Windows:** open the downloaded `.exe` installer and follow the on-screen steps.
- **Linux:** install the DEB/RPM for your distribution, or allow the AppImage to run
  as a program in its file properties and open it.

**These are unsigned beta downloads.** Your computer may show an installation warning:

- **Mac:** after attempting to open Speak, go to **System Settings → Privacy &
  Security** and choose **Open Anyway** for Speak, if offered.
- **Windows:** if SmartScreen appears, choose **More info → Run anyway**, if offered.

Work or school computers may prevent unsigned apps from running.
[More installation details](docs/BETA_INSTALLATION.md).

### 2. Set up your microphone and voice model

1. Allow Speak to use your microphone when asked.
2. On first launch, the **Welcome to Speak** prompt offers **Download and set up**.
   Click it to download the default voice model, about **3.83 GB**.
   Leave extra disk space for the app and any additional downloads.
   Already have model files? Choose **Use existing model files** instead.
3. Wait for download, verification, and startup, then click **Try the test notepad**.
   Interrupted downloads offer **Retry / resume download**. You can choose
   **Continue in background** while setup runs. If you choose **Later** before
   downloading, open **Settings → Or download a verified model → Download / resume**
   whenever you're ready; unfinished setup is offered again on the next launch.
4. **On Mac**, click **Enable keyboard insertion…** and allow Speak in
   **System Settings → Privacy & Security → Accessibility**. This lets Speak put
   your words into other apps. On Linux, approve shortcut/input permissions if asked.

You need an internet connection for the initial download. After setup, you can
dictate offline. Speak chooses the available acceleration automatically.
Installers do not contain the voice model. Existing installations with both model
files present skip the welcome prompt. If you don't see the prompt in an older
version, use the download controls in Settings or install a newer release.

### 3. Hold F5 and talk

1. Click where you want your words to appear, such as a blank note or email.
2. **Hold F5**, wait for the recording indicator, and speak.
3. **Release F5**. Keep the same text field focused while Speak finishes, then
   your text appears.

On Macs where function keys control system features, use **Fn + F5** or choose a
different **Push-to-talk shortcut** in Settings and click **Save settings**.

Prefer using the mouse? **Hold the microphone icon** on the floating control and
release it when you're done. The flowing wave responds to your voice. Use the
six-dot grip to move the control around your screen.

Want to try it first? Click **Open test notepad** in Speak, click inside the note,
and hold F5 or the **Hold to test** button.

## Everyday use

- **Speak naturally.** Speak transcribes your words and tidies punctuation and
  spoken corrections. Review the result, especially names and numbers.
- **Cancel a recording:** click **×** on the floating microphone or **Cancel** in Speak.
- **Change your microphone or shortcut:** open Speak's **Settings** from its menu-bar
  or system-tray icon, make the change, and click **Save settings**.
- **Keep Speak handy:** closing its window leaves it running in the menu bar/system
  tray. Choose **Quit Speak** from that menu to stop it completely.
- **Start with your computer:** enable **Start Speak when I log in** in Settings.

## Need help?

| What's happening?                    | What to try                                                                                                                                                                                                                                      |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| F5 doesn't start a recording         | Try holding the floating microphone. On Mac, try **Fn + F5**, or change the shortcut in Settings.                                                                                                                                                |
| Speak can't hear me                  | Check the **Microphone** choice in Settings and your computer's microphone permissions.                                                                                                                                                          |
| My download stopped                  | Click **Download / resume** again. Speak keeps the partially downloaded files.                                                                                                                                                                   |
| Speak isn't ready                    | Let setup finish. If it reports an error, use **Start / restart Gemma** in Settings. Gemma is the voice model running on your computer.                                                                                                          |
| My words don't appear in another app | Keep the destination focused until processing finishes. On Mac, check Accessibility permission and **Enable keyboard insertion…**. Try the test notepad to check recording separately. Some protected fields or apps won't accept inserted text. |
| Processing failed                    | Use **Retry processing**. You can also copy the raw transcript or use **Save audio** before quitting.                                                                                                                                            |
| It works, but feels slow             | Speed depends on your computer. Close other memory-heavy apps and try **Recheck acceleration** in Settings.                                                                                                                                      |

Still stuck? **[Report a problem](https://github.com/AnishNavali/speak/issues/new)**.
Include your operating system, what you tried, and the error message shown in Speak.

## Privacy and updates

**Your voice is processed on your computer.** Speak contacts the internet to
download its voice model, any needed acceleration files, and updates you request.
It doesn't need a cloud transcription account.

Recordings stay in memory unless you choose **Save audio**. Settings and test
notepad text are saved on your device. Quitting Speak discards any pending recording;
save or copy anything you need first.

To update, open **Application updates → Check for updates** in Settings. Release
builds verify update signatures before installing. You can also download the
latest installer above. Models and settings are kept between updates; Mac users
may need to approve Accessibility again during this unsigned beta period.

## Contributing and demos

- **Building or changing Speak:** see the [developer guide](docs/DEVELOPMENT.md).
- **Preparing a release:** see the [distribution guide](docs/DISTRIBUTION.md).
- **Adding a demo:** see the [demo recording guide](docs/media/README.md).

Speak's application code is [MIT licensed](LICENSE). Voice models and other
components have their own terms; see [third-party notices](THIRD_PARTY_NOTICES.md).
