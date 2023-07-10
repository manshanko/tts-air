# `tts-air`

Library to expose TTS (text-to-speech) events from [Tolk](https://github.com/dkager/tolk/) in Diablo 4.
Used with `tts-air-proxy` TTS events can be proxied to [DButcher](https://d4.wartide.net/app) which has item filtering among other features.

For privacy reasons the WebSocket server in `tts-air-proxy` only accepts local connections from `https://d4.wartide.net` or `localhost`.

## Usage

Download [latest release](https://github.com/ManShanko/tts-air/releases/latest).

The TTS library `saapi64.dll` requires being in the load path for [Tolk](https://github.com/dkager/tolk/).
Placing next to `Diablo IV.exe` or `Tolk.dll` works best.
The parent directory of Diablo 4 can be opened in the `battle.net` app at the Diablo 4 game page with *Options* -> *Show in Explorer* (*Options* is the gear icon next to the play button).

Once set up and in Diablo 4 make sure to run `tts-air-proxy` and enable 3rd party screen reader in Diablo 4 and [DButcher](https://d4.wartide.net/app) should then be able to read TTS events from Diablo 4.

The 3rd party screen reader setting in Diablo 4 requires screen reader to be enabled which requires a compatible Windows Narrator voice installed for the current language selected in Diablo 4.
See [supported languages and voices for Windows Narrator](https://support.microsoft.com/en-us/windows/appendix-a-supported-languages-and-voices-4486e345-7730-53da-fcfe-55cc64300f01).

## Implementation

Diablo 4's 3rd party screen reader support is provided by [Tolk](https://github.com/dkager/tolk/).
Of the supported 3rd party screen readers I tried implementing a few:
* Microsoft Speech API - required an installation step for some registry additions and was too slow
* NVDA - I had performance problems while testing it
* SAAPI - easiest to implement and the one I went with
