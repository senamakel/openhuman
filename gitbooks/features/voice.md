---
description: >-
  Native voice — speech-to-text in, ElevenLabs text-to-speech out, mascot
  lip-sync, and a live Google Meet agent that listens and speaks.
icon: microphone
---

# Native Voice (ElevenLabs)

OpenHuman is voice-first when you want it to be. STT, TTS and the live Google Meet agent are part of the core, not a third-party plugin.

## Speech-to-text

Voice input runs through `src/openhuman/voice/`:

* **Hotkey** (`hotkey.rs`), push-to-talk and toggle modes.
* **Audio capture** (`audio_capture.rs`), cross-platform mic capture with VAD.
* **Streaming transcription** (`streaming.rs`, `cloud_transcribe.rs`), words appear as you speak.
* **Hallucination filter** (`hallucination.rs`), strips the well-known artefacts ("Thanks for watching", silence-induced phrases).
* **Postprocess** (`postprocess.rs`), punctuation, capitalization, dictation cleanup.

Dictation can replace the active text input on your desktop, or be sent straight into a chat with the agent.

## Text-to-speech via ElevenLabs

Reply speech (`reply_speech.rs`) routes through **ElevenLabs**. The agent's responses can be spoken back in a voice you pick, with the timing and prosody you'd expect from ElevenLabs' models.

Key bits:

* Voice selection is configurable per user.
* The TTS client lives in the desktop app at `app/src/features/human/voice/ttsClient.ts`.
* The mascot avatar uses a viseme map (`visemeMap.ts`) to lip-sync to the audio stream.

## Live Google Meet agent

The Meet agent (`src/openhuman/meet_agent/brain.rs`) is OpenHuman's flagship voice integration:

* Joins a Google Meet via the embedded webview.
* Streams audio out to STT in real time, transcribes everyone in the call, and writes structured notes into the [Memory Tree](obsidian-wiki/memory-tree.md) as the meeting progresses.
* When you ask it to speak (or it decides it has something useful to add), it generates audio with ElevenLabs and **plays it back into the meeting as an outbound camera/mic stream**, so other participants actually hear it.

This is real, not a demo: see commits `0bc74575` (live note-taking), `f1203479` (real LLM turns + tuned TTS), `b6d05cb4` (mascot canvas as outbound camera).

## Privacy

* Audio capture is local. Streaming STT goes through the OpenHuman backend; no recording is retained beyond the live transcript.
* TTS audio is streamed and discarded, nothing is stored.
* Meeting transcripts land in your local memory tree, like any other source.

## See also

* [Memory Tree](obsidian-wiki/memory-tree.md). where Meet transcripts and notes live.
* [Automatic Model Routing](model-routing/README.md). Meet's brain uses `hint:fast` for low-latency conversational turns.
