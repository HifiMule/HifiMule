# Share your device profile

A device profile tells HifiMule what your player can handle and how to prepare audio for it. A good profile can preserve compatible files, convert other formats into something your device can play, and provide sensible defaults when setting up a device.

Profiles let you configure:

- **Supported formats:** which audio formats can be copied without conversion, such as MP3, AAC, FLAC, or Opus.
- **Conversion:** download originals, convert incompatible audio, or always convert to your preferred output format and bitrate.
- **Audio limits:** maximum bitrate, sample rate, bit depth, and channel count.
- **Default folders:** where music, playlists, audiobooks, and podcasts should go when setting up a device.
- **Folder artwork:** whether to create `folder.jpg` thumbnails by default.

HifiMule includes **System** profiles, which are bundled with the app and updated with new versions. You can create a **Custom** profile for your own player, or clone a system profile and adjust it. Folder defaults apply during device setup; changing a profile does not move an existing device's folders.

**Found settings that work well with your player? Share them in [HifiMule Discussions](https://github.com/HifiMule/HifiMule/discussions/categories/show-and-tell), under “🙌 Show and Tell”.** Your profile can help someone with the same device get started, and your real-device testing can help us turn it into a built-in system profile in a future HifiMule release.

## 1. Save and test your profile

Open **Device profiles…** from HifiMule's tray menu. You can also use **Edit selected profile…** or **New profile** in a device's profile selector.

Choose your custom profile, give it a recognizable name containing the device brand and model, and click **Save profile**. To adapt a system profile, use **Clone to customize** first.

Select the profile for your device, sync a small selection, and check playback on the player itself. Try the formats and media types you intend to recommend. Note anything you have not tested, along with any limitations such as unsupported playlists, missing artwork, or audiobook chapters that do not work.

## 2. Find `device-profiles.json`

HifiMule stores profiles on your computer in its application data folder:

| Platform | File location |
|---|---|
| Windows | `%APPDATA%\HifiMule\device-profiles.json` |
| macOS | `~/Library/Application Support/HifiMule/device-profiles.json` |
| Linux | `~/.local/share/HifiMule/device-profiles.json`, or `$XDG_DATA_HOME/HifiMule/device-profiles.json` if `XDG_DATA_HOME` is set |

- **Windows:** press **Win + R**, enter `%APPDATA%\HifiMule`, and press **Enter**.
- **macOS:** in Finder, press **Command + Shift + G**, enter `~/Library/Application Support/HifiMule`, and press **Return**.
- **Linux:** open `~/.local/share/HifiMule` in your file manager, or your configured `$XDG_DATA_HOME/HifiMule` folder. Enable hidden files if needed.

If you launch HifiMule with `HIFIMULE_APP_DATA_DIR` configured, look for `device-profiles.json` directly in that directory instead. If the file is missing, launch HifiMule and save your profile first.

This is the profile library on your computer. The `.hifimule.json.zst` file on a synced device is a separate manifest and is not needed for sharing a profile.

## 3. Extract just your profile

1. Make a copy of `device-profiles.json` and open the copy in a plain-text editor.
2. Find the top-level `"profiles"` array, then search for your profile's name. A custom profile normally has `"ownership": "custom"` and an ID beginning with `custom:`.
3. Copy the **complete object** for that profile, from its opening `{` to its matching closing `}`. Include all nested arrays and objects, especially `"deviceProfile"`, which contains the audio rules. Copying only the name or the nested `"deviceProfile"` object loses part of the profile.
4. Paste it into a new file, for example `brand-model-profile.json`. Leave out the comma that separates it from the next profile. The new file should contain one JSON object, without the surrounding `"profiles"` array or the other entries.
5. Keep the profile's fields, including `id`, `name`, `description`, `ownership`, `revision`, folder defaults, and audio settings. Review the name, description, and any custom fields for personal information before posting.

The result should have this shape. This is an illustrative MP3 profile; share your own tested settings:

```json
{
  "id": "custom:example-player",
  "name": "Example Player — MP3 256 kbps",
  "description": "Example settings; replace with your tested profile.",
  "ownership": "custom",
  "revision": 1,
  "defaultMusicFolder": "Music",
  "defaultPlaylistFolder": "Playlists",
  "defaultFolderArtwork": false,
  "deviceProfile": {
    "Name": "HifiMule-Example-Player",
    "MaxStreamingBitrate": 256000,
    "MusicStreamingTranscodingBitrate": 256000,
    "DirectPlayProfiles": [
      { "Container": "mp3", "Type": "Audio", "AudioCodec": "mp3" }
    ],
    "TranscodingProfiles": [
      {
        "Container": "mp3",
        "Type": "Audio",
        "AudioCodec": "mp3",
        "Protocol": "http",
        "EstimateContentLength": true,
        "EnableMpegtsM2TsMode": false
      }
    ],
    "CodecProfiles": []
  }
}
```

JSON bitrate values are in bits per second: `256000` corresponds to **256 kbps** in the editor. Preserve the exported values when sharing.

Share only the extracted profile. The rest of your application data folder, device manifest, and server configuration are unnecessary for this contribution.

## 4. Publish in “🙌 Show and Tell”

Open [HifiMule Discussions](https://github.com/HifiMule/HifiMule/discussions), sign in to GitHub, and start a **New discussion** in **🙌 Show and Tell**. Use a title such as **Device profile: Brand Model — MP3 256 kbps**.

Copy this template into your post, fill in the details, and replace the placeholder in the JSON block with your complete extracted profile:

````markdown
## Device

- Brand and exact model:
- Firmware version (including Rockbox version, if used):
- Connection: USB mass storage / MTP / other
- HifiMule version and computer OS:

## What this profile does

- Formats copied without conversion:
- Conversion format and bitrate:
- Audio limits and default folders:

## Tested on the device

- Formats and media types tested:
- Playlists and folder artwork:
- For audiobooks: M4B playback, chapters, and resume behavior, if tested:
- Known limitations or things not tested:

## Profile JSON

```json
Paste the complete extracted profile object here.
```

## Future inclusion

I'd like this profile to be considered for inclusion as a system profile
in a future HifiMule release.
````

Use GitHub's preview to check that the JSON is readable before publishing. Include what actually worked on your device; a format listed in the settings is not proof that the player supports it. Screenshots are welcome, but include the JSON so others can reuse the settings.

## 5. Use a profile shared by someone else

There is currently no profile import/export button. You can recreate the shared settings through **New profile**, or add the shared JSON manually:

1. Wait for any sync to finish, then choose **Quit** from HifiMule's tray menu and wait for it to exit. Closing the window alone can leave the daemon running.
2. Find your own `device-profiles.json` using the paths above and make a backup.
3. Open it in a plain-text editor. Add the shared profile object as another entry inside the existing `"profiles"` array, with a comma between entries. Keep your existing profiles and all other top-level fields, including storage version and profile mappings.
4. Set the added entry's `"ownership"` to `"custom"` and `"revision"` to `1`. Give it an ID unique within your file, such as `"custom:community-brand-model"`. Do not reuse `passthrough` or a `system:` ID.
5. Save as valid UTF-8 JSON, then restart HifiMule. The profile should appear under **Custom** in **Device profiles**. If the file cannot be loaded, quit HifiMule and restore your backup.
6. Review the settings, select the profile for your device, and test a small sync before using it for a larger transfer. For an already configured device, adjust its folders in device settings if needed; profile folder defaults do not replace existing folders.

Reply to the original discussion with your model, firmware, and results. Confirmations from other owners help us decide which community profiles to include as system profiles. Inclusion requires review and may involve adjustments; posting a profile does not automatically make it a system profile.

**Help the next owner of your player: test your profile, share the JSON in “🙌 Show and Tell”, and tell us what works.**
