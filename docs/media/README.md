# Demo recordings and screenshots

This folder holds the small images/animations shown in the [main README](../../README.md).
There is no demo video committed yet. Send the recording when it is ready; it can
be embedded or linked without changing the app.

## What to record

For a **10–15 second preview**, use a blank note with the floating microphone visible:

1. Click in the note and hold F5 (or Fn + F5 on your Mac).
2. Say: “Let's meet on Friday, sorry, Monday at six.”
3. Show the wave responding while you speak.
4. Release the shortcut and let the finished text appear.
5. Leave the result visible for a moment.

Use the actual result from the app. Corrections depend on the voice model; if the
result needs explaining, use a simpler sentence for the short preview.

An optional **30–45 second walkthrough** can show the model download choice,
microphone/Accessibility permissions, the test notepad, and dictation into another app.

## How to record

- **Mac:** press **Shift + Command + 5**, choose **Record Selected Portion**, and
  frame the note plus floating microphone. In **Options**, choose a microphone if
  you want viewers to hear the spoken sentence. Stop from the menu bar.
- **Windows 11:** use the screen-recording option in **Snipping Tool** and select
  the relevant area. Audio options depend on your Windows/Snipping Tool version.

Keep text readable, hide personal notifications, and use an example note rather
than real messages. A small “Hold F5 → speak → release” caption helps viewers
understand the shortcut without sound. You can send the original MOV or MP4.

## Where the files go

| File | Purpose |
|---|---|
| `docs/media/demo.gif` | Small, silent preview displayed directly in the README |
| `docs/media/setup.png` | Optional screenshot of first-time setup |
| `docs/media/demo.mp4` | Optional small video download, if kept in Git |

These are intended filenames, not files that already exist. Keep a GIF short and
preferably below 10 MB. For a larger video, upload it to a GitHub release or use
GitHub's attachment upload when editing Markdown, then share the resulting URL.
GitHub does not automatically play a repository-relative MP4 Markdown link as an
inline video; an attachment URL is better for that.

After `demo.gif` exists, put this below the introduction in the root README:

```markdown
## See Speak in action

![Speak recording a sentence and inserting the finished text into a note](docs/media/demo.gif)
```

For `setup.png`, add this beside the setup instructions:

```markdown
![Speak Settings showing the default model download and microphone controls](docs/media/setup.png)
```

Only add the image/video references once their files or attachment URLs are
available, so visitors never see a broken demo.
