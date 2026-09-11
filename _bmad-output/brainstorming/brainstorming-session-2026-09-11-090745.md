---
stepsCompleted: [1, 2]
inputDocuments:
  - README.md
  - docs/architecture-hifimule-daemon.md
  - docs/integration-architecture.md
session_topic: 'Add playback to HifiMule as an iTunes-like companion for self-hosted music servers'
session_goals: 'Explore the best audio experience, assess Rust constraints, and decide where playback belongs for consistency and responsiveness'
selected_approach: 'progressive-flow'
planned_techniques: ['What If Scenarios', 'Mind Mapping', 'SCAMPER Method', 'Decision Tree Mapping']
techniques_used: ['What If Scenarios']
ideas_generated: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46]
context_file: ''
---

# Brainstorming Session Results

**Facilitator:** Alexis with Codex
**Date:** 2026-09-11

## Session Overview

**Topic:** Extend HifiMule with music playback alongside library browsing and portable-device sync.

**Goals:** Define an excellent listening experience; explore Rust feasibility and playback placement; examine consistency and latency without prematurely choosing an architecture.

### Context Guidance

The README describes a detachable Tauri UI and a Rust daemon running on the desktop, with external Jellyfin and Subsonic-compatible servers providing music. Generated architecture documents provide background but may lag the implementation.

### Session Setup

The user explicitly requested a new session. They favor playback outside the UI but remain uncertain. The meaning of server-side playback (local HifiMule daemon versus remote media server), intended listening outputs, and quality priorities remain open. No playback architecture or library has been selected. Technique approach selection is pending.

### Clarifications

The user clarified that server-side means the local Rust daemon. They approved a progressive flow: ideal listening experience, alternatives, then architecture. This resolves the location terminology; quality and latency advantages remain hypotheses to evaluate.

## Technique Selection

**Approach:** Progressive Technique Flow, approved by the user.

- **Exploration — What If Scenarios:** Describe ideal listening sessions and expand possibilities without committing to a playback implementation.
- **Pattern recognition — Mind Mapping:** Connect needs emerging from the dialogue when the user is ready to organize.
- **Development — SCAMPER Method:** Refine promising experiences and challenge assumptions about playback responsibilities.
- **Action planning — Decision Tree Mapping:** Turn priorities and verified technical constraints into implementation choices and experiments.

Pace follows the conversation. Begin with one ideal-listening scenario; user may change direction at any point. Technical feasibility research will follow concrete requirements.

## Exploration: Listening Scenarios

**[Discovery #1]: Listen Before Choosing**
_Concept_: Preview tracks or albums while browsing, before adding them to a playlist or device sync basket. Playback supports music selection within the existing HifiMule workflow.
_Novelty_: Connects listening directly to portable-library curation.

**[Everyday Listening #2]: One Desktop Music Companion**
_Concept_: Use HifiMule for everyday playback on the main PC or Mac in place of a separate Jellyfin or Navidrome client. Users already install it for device sync.
_Novelty_: Extends the existing multi-server sync companion into the user's everyday player.

**[Background Listening #3]: Auto-Fill for Listening**
_Concept_: While working with headphones, generate background listening from a playlist, genre, artist, or similar starting point. Explore reusing the existing track-selection algorithm used for device sync.
_Novelty_: Applies the same selection intelligence to both portable listening and desktop playback. Whether the queue is finite or continually replenished remains open.

**[Album Listening #4]: Preserve the Album Sequence**
_Concept_: Listen through hi-fi speakers with tracks in album order and gapless transitions. Preserve the album's intended continuity; detailed handling of silence within recordings remains to be clarified.
_Novelty_: Treats the album as a continuous listening experience rather than unrelated tracks. No automatic crossfade requirement was expressed.

### Next Exploration Prompt

Explore how previewing a candidate track interacts with an already-playing background queue. A temporary audition that returns to the prior session is an assistant proposal, not an accepted requirement.

**[Discovery #5]: Temporary Audition, Preserved Session**
_Concept_: The user accepted temporary auditioning of a candidate track while preserving the previous track, playback position, and queue, with a way to return to the listening session. Previewing supports curation without discarding ongoing background listening.
_Novelty_: Gives auditioning its own temporary context alongside the main listening session. Automatic versus explicit return, preview duration, and nested previews remain unspecified.

### Next Exploration Prompt: Background Queue Lifetime

Explore whether auto-fill-inspired listening should produce a finite queue or keep replenishing while the user works. Neither behavior has been selected yet.

**[Background Listening #6]: Continuous Library Radio**
_Concept_: The user selected a Radio experience that keeps replenishing the playback queue until explicitly stopped. Starting points include a playlist, genre, or artist; the earlier proposal to reuse auto-fill selection remains the foundation to explore.
_Novelty_: Turns portable-device selection intelligence into an ongoing desktop listening session. Repetition policy, selection boundaries, and handling a small or exhausted candidate pool remain open.

### Next Exploration Prompt: Radio Discovery Boundaries

Explore whether an artist seed stays within that artist or expands to related music in the user's connected library. Relatedness signals and technical feasibility are not yet specified.

**[Radio Selection #7]: Strong Musical Connections**
_Concept_: The user wants Radio to stay close to the current artist while allowing other artists through obvious connections stronger than shared genre alone. Collaborations, shared performers, and band membership were suggested as possible signals, but specific signals and their metadata sources have not been accepted or validated.
_Novelty_: Requires meaningful musical relationships for transitions rather than treating a broad genre label as sufficient similarity.

**[Radio Selection #8]: A Moving Artist Center**
_Concept_: When all tracks by the current artist have been heard, Radio may move to a connected artist as its new center instead of repeating the original artist to preserve proximity. The user welcomed cumulative movement as a musical journey rather than requiring a permanent anchor to the starting artist.
_Novelty_: Maintains local musical coherence while allowing the station to evolve over time. The scope of heard-track history and the precise timing of a transition remain open.

**[Library Context #9]: Rediscovery Within a Curated Collection**
_Concept_: The user emphasized that available music has already been acquired and accepted by the listener or their family. Radio explores an existing curated collection, so gradual travel through it is welcome; individual and family preferences need not be identical.
_Novelty_: Positions Radio as a way to rediscover owned music rather than as discovery across an unrestricted streaming catalog.

### Next Exploration Prompt: Meaning of a Skip

Explore whether a skip is local to the moment or should influence future Radio selections. No adaptive preference learning has been requested or accepted yet.

**[Playback Feedback #10]: Skip Without Local Taste Learning**
_Concept_: The user regards a skip as rejection of that track in the moment but explicitly does not want a persistent taste-memory layer in HifiMule. Where possible, a skipped track should not be recorded as played on the underlying server.
_Novelty_: Separates a playback action from durable preference learning. Provider reporting semantics, reporting thresholds, and whether an already-reported play can be avoided or reversed require verification.

**[Playback Feedback #11]: Server-Owned Explicit Preferences**
_Concept_: Offer explicit Like / Dislike actions whose track status is stored in the underlying music server and available beyond HifiMule. This is a user proposal; support and equivalent semantics across providers remain unverified.
_Novelty_: Keeps lasting preferences with the library source instead of introducing a HifiMule-only profile. Removing a favorite must not silently be treated as an explicit dislike.

**[Radio Session #12]: Temporary Skip Exclusion**
_Concept_: The user approved remembering skipped tracks within the current Radio session to prevent immediate reselection. This temporary history disappears when the session ends and does not become a persistent taste model.
_Novelty_: Prevents repetitive Radio behavior while respecting the user's preference against local taste memory. What constitutes the end of a session remains unspecified.

### Exploration Domain Pivot

After developing Radio selection and feedback, return to hi-fi album listening. Next clarify the listener's actual output equipment before proposing audio-quality behavior or implementation. No Rust audio stack, device mode, or quality guarantee has been selected.

### Hi-Fi Output Context and Initial Research

The user's speakers connect through a professional USB audio interface supporting Windows audio and ASIO. They asked whether ASIO matters for listening; the interface model remains unspecified.

ASIO offers low-latency audio access, useful especially for recording and monitoring. Microsoft documents WASAPI shared mode through the Windows audio engine and exclusive mode with direct device access, bypassing that engine. Working hypothesis: ASIO need not be a playback requirement solely for fidelity; evaluate WASAPI and the actual interface before selecting a backend. No claim of guaranteed bit-perfect output or audible improvement is established.

Sources:
- https://download.steinberg.net/downloads_software/WaveLab_Cast_2/help/en/wavelab/topics/setting_up_your_system/asio_driver_c.html
- https://www.steinberg.net/tutorials/setup-your-audio-interface/
- https://learn.microsoft.com/en-us/windows/win32/coreaudio/user-mode-audio-components

Next user-facing question: should dedicated album listening take exclusive ownership of the selected output, preventing other applications from using that endpoint, or coexist with their audio? This is an exploration, not an accepted requirement.

**[Audio Output #13]: Shared Output, OS-Owned Interruptions**
_Concept_: The user prefers shared output as the general default so other applications, notifications, and ringing can still sound. Do Not Disturb remains the OS's responsibility.
_Novelty_: Defines high-quality playback around coexistence and everyday usability rather than assuming exclusive device ownership. Shared mode may involve resampling or configured effects; bit-perfect output is not promised. ASIO and exclusive mode are not established requirements.

**[Audio Output #14]: Pause on Output Loss**
_Concept_: The user approved pausing when the active audio device disconnects rather than automatically sending music to another output. This prevents unexpected playback through computer speakers.
_Novelty_: Makes output loss a deliberate pause in the listening session. Reconnection and manual output-switching behavior remain open.

### Next Exploration Prompt: Detachable UI

Return to the original daemon-playback motivation: clarify whether closing the HifiMule window should leave playback running and accessible through system media controls. This behavior has been discussed as a possibility but not yet explicitly accepted.

**[Desktop Playback #15]: Music Without the Window**
_Concept_: The user wants playback to continue after closing the UI, with keyboard media control and low memory consumption. This is a desired outcome conditional on feasibility, not a verified implementation capability.
_Novelty_: Makes the detachable UI architecture serve listening as well as sync. Releasing UI memory and integrating media controls independently on Windows and macOS require investigation; no memory budget has been set.

**[Radio Interaction #16]: An Editable Local Playlist**
_Concept_: The user wants Radio to follow the familiar auto-sync playlist model so its tracks can be seen and edited. The active Radio playlist is kept locally and replenished during playback.
_Novelty_: Makes algorithmic listening inspectable and editable through an existing product concept. Respecting manual ordering during replenishment was proposed by the assistant but exact edit semantics remain open. Local queue state is distinct from the rejected persistent taste-memory layer.

**[Playlist Ownership #17]: Explicit Save to Server**
_Concept_: The user selected local Radio playlist ownership with an option to save it to the underlying music server. Automatic server synchronization was not requested.
_Novelty_: Offers deliberate reuse outside HifiMule without requiring a server playlist for each Radio session. Whether saving includes played history, upcoming tracks, or both remains open; cross-server track handling also remains open.

### Next Exploration Prompt: Save Scope

Clarify what a user expects to preserve when saving an evolving Radio playlist: the musical journey already heard versus its upcoming queue. No snapshot contents or ongoing synchronization semantics have been selected.

**[Playlist Snapshot #18]: Preserve the Accepted Journey and Upcoming Queue**
_Concept_: The user defined the saved playlist as its current state: already-played tracks, excluding skipped or disliked tracks, plus upcoming tracks. Saving captures the listening journey as well as what is queued next.
_Novelty_: Retains the session's useful discoveries while leaving rejected selections out of the saved result. Treatment of the currently playing track and save-time races remains unspecified.

**[Radio Queue #19]: A Limited Lookahead**
_Concept_: Build only a limited number of upcoming tracks and replenish as listening progresses. Do not populate the playback queue with the entire library.
_Novelty_: Keeps an ongoing Radio session manageable and editable while leaving later selections open. No track count or user-facing setting has been selected; candidate discovery is separate from queue materialization.

**[Multi-Server Playback #20]: Cross-Library Radio**
_Concept_: The user proposed that Radio draw from multiple configured music servers. A single local listening session could travel across those curated libraries.
_Novelty_: Extends HifiMule's multi-server identity to listening, beyond a player attached to one library. Source selection, duplicate recordings, availability, metadata connections, and export of mixed-server playlists remain open.

### Next Exploration Prompt: Mixed-Server Playlist Export

Discuss the product behavior for saving a Radio playlist whose tracks come from multiple servers. Do not assume one server can reference another server's track identifiers. Provider capabilities need verification before implementation; one candidate is keeping the complete local playlist and explicitly saving the chosen server's subset with a clear omitted-track count.

**[Playlist Export #21]: Split Saves Across Contributing Servers**
_Concept_: The user chose to save a mixed-server playlist as one playlist on each contributing server, containing that server's portion. The assistant clarified that each portion retains relative order and the local playlist retains the full sequence.
_Novelty_: Makes an explicit save usable across HifiMule's multiple sources without assuming foreign track references are supported. Naming, partial failures, and duplicate-source assignment remain open.

**[Recording Identity #22]: Deduplicate Across Servers**
_Concept_: The user confirmed that copies of the same recording on multiple servers should count as one track for Radio, avoiding duplicate playback. This does not establish that different performances or editions should be merged.
_Novelty_: Treats the listening collection as recordings across sources rather than as a concatenation of server track IDs. Matching confidence, version distinctions, preferred playback source, and fallback behavior remain open.

### Next Exploration Prompt: Return to Audio Fidelity

After the multi-server branch, explore whether playback should request the original audio by default, including when a lower-quality copy exists for portable-device sync. This is a proposed requirement, not a selected behavior. Adaptive streaming and bandwidth constraints remain open.

**[Playback Quality #23]: Highest Sustainable Quality**
_Concept_: The user wants the best available audio quality for desktop playback, independently of portable-device sync formats. When the server connection is poor, playback should select the best quality it can deliver without interruptions.
_Novelty_: Makes reliable listening the constraint on quality selection rather than using a fixed portable-device profile. Provider transcoding support, buffering, quality-ranking semantics, switching boundaries, and recovery to higher quality require investigation. Seamless mid-track switching has not been established as feasible, and an outage cannot be guaranteed interruption-free.

### Next Exploration Prompt: Automatic Quality Adaptation

Clarify whether HifiMule should autonomously lower quality when needed and recover when possible, with a visible explanation, or ask the listener first. No automatic adaptation policy has yet been accepted.

**[Playback Quality #24]: Automatic Adaptation for Ease of Use**
_Concept_: The user approved automatic quality adjustment without asking first, emphasizing ease of use. The proposed behavior includes discreet explanation of reduced quality and recovery to higher quality when conditions permit.
_Novelty_: Keeps streaming management out of the normal listening flow. Exact recovery policy and provider-specific feasibility remain open.

**[Playback Reliability #25]: Buffer-Informed Quality Selection**
_Concept_: The user accepts a brief startup buffer and proposes using its behavior to assess the quality the connection can deliver. Buffering should protect continuity and inform automatic selection of sustainable quality.
_Novelty_: Connects the reliability mechanism with quality adaptation instead of asking the listener to estimate network capability. Buffer targets, throughput estimation, safety margin, and protection against frequent quality oscillation remain to be designed and tested; buffering alone does not guarantee uninterrupted playback.

### Next Exploration Prompt: Network Failure Behavior

Explore whether Radio may skip an unavailable track and continue with a reachable source, while ordered album playback pauses and retries rather than silently omitting a track. This distinction is an assistant proposal awaiting user input.

**[Playback Reliability #26]: Recovery Respects Listening Mode**
_Concept_: The user approved continuing Radio with another available track when a server is unreachable, while album playback pauses and retries to preserve track order. Recovery should follow the listener's intent for the session.
_Novelty_: Distinguishes exploratory listening from faithful album playback. A technical failure is not an explicit user skip or dislike; retry timing, exhaustion of all available sources, and duplicate-source fallback remain unspecified.

### Next Exploration Prompt: Listening Comfort

Shift from network reliability to perceived volume across recordings: explore automatic loudness matching for Radio while preserving relative track levels within album playback. This is a product proposal; metadata availability and processing choices have not been investigated.

**[Listening Comfort #27]: Loudness Matching by Listening Mode**
_Concept_: The user approved automatic loudness matching between recordings in Radio, while preserving intentional relative loudness differences between tracks during whole-album playback. The aim is comfortable background listening and faithful album presentation.
_Novelty_: Applies volume consistency at the appropriate musical scope instead of independently leveling every album track. Metadata availability, gain calculation, peak protection, fallback for missing information, and user overrides remain unverified design details. No dynamic-range compression requirement was expressed.

### Next Exploration Prompt: Playback and Device Sync Together

Explore what should happen when automatic device sync begins during listening and competes for network or compute resources. Giving playback priority and slowing sync is an assistant proposal, not yet an accepted requirement.

### Playback Isolation Proposal and Evidence

The user proposed a dedicated playback thread to prevent sync from affecting playback, comparing this to existing UI isolation. Source inspection shows the UI is launched as a separate process (`main.rs`), while daemon work uses a multi-thread Tokio runtime and sync uses asynchronous tasks (`sync.rs`). An async task is not a dedicated audio thread.

Proposed audio design: download/decode workers feed a bounded audio buffer consumed by a dedicated audio callback thread. Keep blocking I/O, allocations, and locks shared with sync out of that callback. CPAL documents dedicated high-priority callbacks on modern platforms, showing this pattern is available in Rust without committing to CPAL as the final stack.

Thread separation does not isolate network bandwidth, disk, CPU capacity, or server transcoding resources. Protecting the playback buffer may additionally require limiting sync demand. This resource-priority policy remains a recommendation awaiting user agreement.

References:
- https://docs.rs/cpal/latest/cpal/
- https://portaudio.com/docs/v19-doxydocs/writing_a_callback.html

**[Playback Isolation #28]: Independent Audio Delivery, Conditional Sync Backoff**
_Concept_: The user accepted a dedicated audio thread fed by buffering, with playback and sync running normally together. Reduce sync demand only when needed to protect playback, rather than slowing sync whenever music is playing.
_Novelty_: Responds to actual contention instead of assuming it. The user reports measurements across scenarios showing sync is often limited by portable-device bandwidth rather than server-to-HifiMule transfer; those measurements were not independently inspected in this session. Buffer-risk detection and attribution to sync remain design work.

**[Validation #29]: Gapless Playback During Large Sync**
_Concept_: The user approved verifying gapless album playback during a large device sync, checking both audio dropouts and sync throughput. This test should assess coexistence under the workload that defines HifiMule.
_Novelty_: Validates playback in the presence of the product's existing main workload, rather than only testing an idle player. No test has been implemented or run during brainstorming.

### Next Exploration Prompt: Turning Listening into Portable Music

Return from concurrency details to the user's original curation use case. Explore a session action to add Radio discoveries to the sync basket using the same played-minus-rejected-plus-upcoming snapshot semantics. This is a proposal, not an accepted requirement.

**[Listening to Sync #30]: Add or Replace a Connected Device's Basket**
_Concept_: The user approved sending the current Radio playlist snapshot to the sync basket as an alternative to saving a playlist. Offer two explicit actions: Add to the basket and Replace the basket. A device must be connected for either basket action to be available.
_Novelty_: Bridges listening and portable-device curation while preserving a deliberate choice between extending and replacing a device's existing selection. Snapshot semantics are played tracks minus skipped/disliked tracks plus upcoming tracks. Selection among multiple connected devices, interaction with active sync, and presentation when no device is connected remain open. No persistent live link from Radio to the basket was requested.

### Next Exploration Prompt: Session Continuity

After thirty collaboratively developed ideas, pivot from export/sync operations to continuity across application restarts. Explore whether the local queue and playback position survive a full daemon shutdown while remaining paused on next launch. This must remain distinct from temporary skip history and persistent taste learning; lifecycle details are not yet agreed.

**[Session Continuity #31]: Restore Ready to Resume**
_Concept_: The user approved restoring the local playback queue and track position after fully quitting HifiMule or restarting the computer. Playback remains paused until the listener resumes.
_Novelty_: Preserves listening continuity without unexpected sound on launch. This is playback state, not a persistent taste profile. Whether a restored Radio is the same session for temporary skip exclusions remains unresolved.

### Next Exploration Prompt: Scope of Restored Radio State

Clarify whether restoring Radio also continues its temporary skipped-track exclusions, which were previously defined to disappear when the session ends. Proposed interpretation: explicit new Radio or clearing the session ends those exclusions; restarting the app merely suspends and restores the session. This interpretation requires user agreement.

**[Session Continuity #32]: Restore Session-Scoped Skip Exclusions**
_Concept_: The user approved retaining skipped-track exclusions when restoring the same Radio session. Starting a new Radio clears those exclusions; application restart alone does not end the logical listening session.
_Novelty_: Preserves coherent Radio behavior across restarts without accumulating a cross-session taste profile. This refines the earlier temporary-history requirement: temporary refers to the lifetime of the Radio session rather than the daemon process.

### Facilitation Checkpoint

Thirty-two ideas have emerged through dialogue. Invite the user to identify missing or frustrating aspects of existing players rather than continue only with assistant-led yes/no proposals. Keep exploration open; no implementation scope or final architecture has been selected.

**[Product Motivation #33]: Resolve the Blank-Page Listening Problem**
_Concept_: The user identified deciding what to start listening to as the most annoying aspect of existing clients. Reusing HifiMule's sync selection engine to build a playlist on the go is appealing because it relieves that initial choice.
_Novelty_: Makes getting from indecision to music a central playback outcome, beyond merely adding transport controls. Requiring an artist, genre, or playlist seed every time may leave the original problem unresolved.

### Next Exploration Prompt: Start Without a Seed

Propose a prominent Play something action that selects an opening track from the connected curated collection using existing auto-fill preferences and starts an evolving Radio session. No new local taste profile is implied. Ask about the desired character of that opening selection before treating this proposal as accepted.

**[Listening Entry #34]: Start with Auto-Fill's First Selection**
_Concept_: The user specified that the first track the existing sync selection engine would choose is a good starting point for Play something. Radio then evolves from that starting track without requiring an explicit artist, genre, or playlist seed.
_Novelty_: Resolves the blank-page problem using existing selection behavior rather than adding a second preference system. Which saved auto-fill preferences apply when no device is connected remains open.

**[Desktop Entry #35]: Play Something Without Opening the UI**
_Concept_: The user proposed adding Play something to the existing app-icon menu, described as the dock button menu currently offering Open the UI. This should start listening directly through the daemon without launching the main window.
_Novelty_: Connects the blank-page solution with lightweight background playback: selecting one menu action starts music. Exact macOS Dock versus menu-bar and Windows tray integration requires verification; this session has not established the existing menu's full contents. Behavior when a session already exists or setup is missing remains open.

### Next Exploration Prompt: Controls Without the Window

Explore a contextual Play/Pause and Next alongside Play something so a listener can control an existing session without opening the UI. Play something would start a fresh Radio, while Resume would continue the existing session; that distinction is an assistant proposal awaiting agreement.

**[Desktop Entry #36]: Separate Fresh Radio from Resume**
_Concept_: The user explicitly approved distinguishing Play something, which starts a fresh Radio, from Resume, which continues the existing queue and playback position. Both support listening without opening the main window.
_Novelty_: Avoids confusing a fresh automatic selection with session restoration. Other contextual menu controls and replacement behavior during active playback have not been explicitly selected.

### Next Exploration Prompt: Device-Independent Selection Defaults

Clarify how Play something chooses selection preferences without a connected device. Reuse of the selection algorithm does not inherently define which device's saved preferences to use. Explore built-in playback defaults with optional customization rather than silently following whichever device was most recently synced.

**[Playback Configuration #37]: A Built-In Virtual Playback Device**
_Concept_: The user proposed a virtual device named Playback that shares the selection engine with sync while owning independent settings. Configure server sources for automatic listening and a playlist for manually selected tracks through this device model.
_Novelty_: Reuses a familiar HifiMule configuration concept without coupling playback preferences to a physical device. The assistant proposed treating the playback queue as the destination instead of file transfer and omitting irrelevant storage/transfer settings. Exact manual versus automatic queue interaction remains open.

**[Device Navigation #38]: Playback First and Always Available**
_Concept_: The user explicitly wants Playback in the existing device selector, listed first. Select it when no physical devices are connected.
_Novelty_: Gives listening a permanent place in the current navigation and a useful default without hardware attached. Arrival of a physical device, preservation of playback while changing selection, and how basket terminology applies to the virtual device remain unresolved. Earlier connected-device requirements for physical sync basket actions are not automatically removed by this proposal.

### Next Exploration Prompt: Physical Device Arrival

Clarify whether connecting a physical device while Playback is selected should change the visible selection or leave Playback selected until the listener chooses the physical device. Audio continuity and background auto-sync should be considered independently from the selected device in the UI.

**[Device Feedback #39]: Show the Connected Device Without Interrupting Music**
_Concept_: The user wants connecting a configured physical device to select it and show its basket, providing immediate feedback for an intentional user action. The user accepted continued music playback, return to Playback when the last physical device disconnects, and visible feedback when a detected device cannot be opened.
_Novelty_: Separates navigation focus from the active listening session while making hardware recognition evident. The user specifically cited frustration when iTunes fails to recognize an attached iPod. Exact selection behavior among multiple remaining physical devices is unspecified.

**[Device Setup #40]: Keep Blank-Device Configuration Visible**
_Concept_: The user explicitly requires a visible configuration option when a blank or uninitialized physical device is connected. The permanent Playback device must not conceal the existing device-setup entry point.
_Novelty_: Ensures the new listening default does not make newly attached devices appear ignored. Setup should remain an explicit action; automatic initialization was not requested.

### Next Exploration Prompt: Playback Controls Across Device Views

Explore persistent compact playback controls while managing a physical device or configuring a blank one. These controls would belong to the ongoing listening session, independently of the currently selected device; exact layout and controls are not yet agreed.

**[Playback UI #41]: A Translucent Floating Player Below the Browser**
_Concept_: The user suggested positioning the player under the media browser, then specified a transparent floating treatment reminiscent of Apple Music so the bottom of the list remains visible. The player should support ongoing listening while device management changes elsewhere in the window.
_Novelty_: Keeps playback accessible without dedicating a large opaque region to transport controls. The assistant proposed artwork, track information, transport, seeking, volume, and a queue shortcut; exact controls remain to be refined. Always-visible versus session-only visibility was not resolved by the user's reply. Ensure final rows can scroll clear of the overlay and remain actionable, with readable controls; these are proposed design safeguards, not a completed design.

### Next Exploration Prompt: Idle Player Entry Point

Clarify the still-open empty-state behavior by proposing an idle Play something action in the floating bar, directly addressing the user's blank-page frustration. Do not treat the earlier affirmative response to floating appearance as a visibility-policy decision.

**[Playback UI #42]: An Idle Invitation to Play**
_Concept_: The user explicitly approved keeping the floating player visible while idle, offering Play something. The main window provides the same low-effort listening entry point as the app-icon menu.
_Novelty_: Turns the player's empty state into a direct response to the blank-page problem. Presentation of a paused restored session is distinct from having no session and remains to be designed.

### Next Exploration Prompt: Preview Versus Play

Return to auditioning during curation. Explore an explicit Preview action separate from Play, with a visible return-to-session control, so browsing a candidate does not accidentally replace the preserved Radio or album queue. No track-row gesture or action mapping has been selected.

**[Preview Entry #43]: Audition Beside Curation Actions**
_Concept_: The user suggested a preview button near the plus action used to add music to the basket, or a contextual-menu entry near Add to playlist. The assistant proposed offering both and showing preview status with a return-to-session control in the floating player.
_Novelty_: Places auditioning at the point where the listener decides what to keep. Final placement and whether both entry points ship remain open; the user offered alternatives rather than explicitly selecting both.

**[Preview Behavior #44]: Full Tracks, User-Controlled Switching**
_Concept_: The user wants previews to play the full track rather than a timed excerpt, until stopped or another track is played. This extends the earlier accepted temporary audition model with a preserved main listening session.
_Novelty_: Supports evaluating the complete recording without a forced preview cutoff. The proposed interpretation is that successive previews replace the current audition while retaining the same main session; natural-end behavior and whether preview listens are reported to the server remain open.

### Next Exploration Prompt: Natural End of a Preview

Clarify whether a preview finishing naturally automatically resumes the preserved Radio or album session, or stops and waits for an explicit return. Do not infer automatic return from the full-track requirement.

**[Preview Continuity #45]: Resume After the Preview Ends**
_Concept_: The user explicitly chose automatic resumption of the preserved listening session when a preview reaches its natural end. Resume the original track at its saved position with its queue intact.
_Novelty_: Completes the temporary audition loop without requiring another action after a full-track preview. Explicit stop behavior, a previously paused main session, and previewing with no prior session remain unspecified.

### Next Exploration Prompt: Preview Listening History

Explore whether a fully heard preview should count as a listen on the underlying server, following the earlier preference not to count skipped tracks as played where possible. Provider reporting semantics remain unverified.

**[Preview History #46]: Report Completed Auditions Where Supported**
_Concept_: The user approved counting a fully heard preview as a listen on the underlying server, depending on server capabilities. An interrupted preview follows the earlier skipped-track reporting intent: avoid counting it as played where possible.
_Novelty_: Makes genuine listening during curation available to the source library's history without adding a separate HifiMule taste profile. Reporting thresholds, seek behavior, partial-play reporting, and deduplication across resume still need provider-specific investigation.

### Facilitation Checkpoint

The preview branch now covers entry points, full-track duration, session preservation, natural-end resumption, and reporting intent. Invite the user to continue experience exploration or return to the original Rust/daemon feasibility question. Technical investigation is a possible direction, not a completed architecture or implementation commitment.

## Initial Feasibility Assessment — 2026-09-11

The user explicitly requested switching from experience exploration to feasibility. The following is based on targeted repository reads and current primary documentation. It is a preliminary assessment, not a validated prototype or final architecture; no application code was changed and no audio tests were run.

### Overall Assessment

Rust and daemon-owned playback are viable. The core listening experience has established building blocks. Main risks lie in codec/container-specific gapless behavior, changing transcoded streams mid-track, OS session integration, and metadata quality for meaningful artist transitions and deduplication.

### Reuse and Required Changes

- `auto_fill/pipeline.rs::run_pipeline` is a pure function over supplied candidates, configuration, history, and a random seed. This is a strong reuse boundary for sync and Radio.
- The current budget is byte/duration oriented; Radio needs an explicit track lookahead and session exclusions. Repeated selection must avoid re-emitting the same deterministic first result and preserve manual edits.
- Inputs currently materialize candidate pools. A bounded playback queue alone does not bound library-index memory or network fetching. Cache/index candidates and measure memory on large libraries rather than rebuilding whole libraries for each replenishment.
- `Candidate`, `SourceKey`, and history in the pure engine use song/source identifiers without an explicit server field. Playback aggregation must preserve `(server_id, track_id)` identities or introduce an equivalent collision-safe mapping. Existing multi-server sync orchestration does not prove that merging raw candidate pools is safe.
- `Song` currently lacks recording MusicBrainz IDs, performer relationships, ReplayGain, sample rate, and bit depth. Extend provider metadata deliberately instead of relying on artist/title and bitrate alone.
- Existing quality/version collapse is heuristic. Cross-server same-recording identity must not silently collapse distinct performances. Keep recording identity separate from choosing among encodings or masters.
- Present Playback as a virtual device in the UI, but use a typed playback destination internally; do not simulate a filesystem mount or route it through destructive file-sync operations. Share selection and appropriate configuration components.

### Audio Engine and Gapless Playback

Candidate design: asynchronous fetching and decoding workers feed a bounded PCM buffer; a dedicated audio callback consumes ready samples. Queue/session control, metadata, RPC, and sync run outside the audio callback. Preload the next track and join decoded samples without closing the output stream; preserve recorded silence while removing known encoder padding. Handle output sample-rate/channel conversion explicitly.

CPAL offers native output and dedicated high-priority callbacks on modern platforms. Symphonia offers Rust decoding, but its current support table distinguishes container and codec gapless support: FLAC/MP3/PCM/Vorbis are supported at the codec level, whereas AAC-LC and ISO/MP4 gapless support are marked absent. ALAC decoding support alone does not establish gapless M4A playback. Rodio offers a higher-level alternative, but its decoder options cannot eliminate underlying format limitations. Prototype representative files before choosing a stack; evaluate an alternative decoder/backend if required formats fail.

Sources: [CPAL](https://docs.rs/cpal/latest/cpal/), [Symphonia support matrix](https://docs.rs/symphonia/latest/symphonia/index.html), [gapless implementation guidance](https://github.com/pdeljanov/Symphonia/discussions/169), [Rodio decoder](https://docs.rs/rodio/latest/rodio/decoder/), [callback constraints](https://portaudio.com/docs/v19-doxydocs/writing_a_callback.html).

### Adaptive Quality

Best sustainable quality is feasible as a policy, but arbitrary seamless mid-track changes are not established. OpenSubsonic supports bitrate/format requests; audio start offsets depend on the `transcodeOffset` extension. Requesting a new bitrate starts a new stream, not an automatic seamless switch of an existing one. Jellyfin supports transcoding, but its exact audio negotiation and transition behavior need a provider-specific spike.

Start with measured buffered duration, refill rate, and conservative headroom. Prefer changes at track boundaries where possible; test mid-track alignment and restart latency independently. Local transcoding after downloading the original does not reduce server-to-client bandwidth. If the server cannot produce a suitable stream and the connection is insufficient, continuous playback cannot be guaranteed. Rank encodings using format-aware metadata, not bitrate alone.

Sources: [OpenSubsonic stream](https://opensubsonic.netlify.app/docs/endpoints/stream/), [audio transcode offsets](https://opensubsonic.netlify.app/docs/extensions/transcodeoffset/), [Jellyfin transcoding](https://jellyfin.org/docs/general/post-install/transcoding/).

### OS Integration and Memory

Souvlaki documents macOS media control support with an application event loop and no open window, and Windows support requiring an HWND. A daemon-owned native window/event integration should be prototyped independently of the Tauri WebView. Verify process exit when the main UI closes rather than assuming hiding a window releases its memory.

The repository retains an optional Windows service path running as LocalSystem. Windows services run in session 0 and cannot directly interact with the logged-in user's desktop. Prefer playback in the user's tray/background process; if service deployment is retained, design a user-session companion. This is a deployment qualification, not evidence that all current installations use service mode.

Bound compressed prefetch and decoded buffers separately. For scale, ten seconds of stereo 48 kHz float32 PCM is 3,840,000 bytes before other overhead. No low-memory guarantee is established until measuring decoder, artwork, candidate pools, and process footprints together.

Sources: [Souvlaki requirements](https://docs.rs/crate/souvlaki/latest), [Windows service sessions](https://learn.microsoft.com/en-us/windows/win32/services/interactive-services). Local evidence: `service.rs` service configuration and `main.rs` UI process launch.

### Radio Relationships, Loudness, and Feedback

Artist drift with explainable relationships is the largest data uncertainty. MusicBrainz exposes performer and artist relationships; use authoritative IDs and cache fetched relationships restricted to music actually available in configured libraries. This factual metadata cache is distinct from a taste profile. Coverage on the user's actual collection must be measured. Subsonic's similar-song endpoint returns similarity candidates but does not itself establish the strong relationship explanations requested. Do not quietly fall back to shared genre alone.

ReplayGain provides track and album gain/peak fields in OpenSubsonic. Use track gain for Radio and a consistent album gain for album playback, with peak protection. Missing tags need an explicit fallback; loudness analysis would add work and resource use.

Playback reporting and preferences require provider capability mapping. OpenSubsonic separates now-playing notifications from submitted listens. Its ratings are 1–5, with 0 removing a rating; this is not a universal boolean dislike. Audit Jellyfin and each supported server before promising uniform Like/Dislike semantics or skip suppression. Split playlist export is ordinary provider work but requires reporting partial successes and safe retries.

Sources: [MusicBrainz relationships](https://musicbrainz.org/relationships), [MusicBrainz API](https://musicbrainz.org/doc/MusicBrainz_API), [similar songs](https://opensubsonic.netlify.app/docs/endpoints/getsimilarsongs/), [ReplayGain](https://opensubsonic.netlify.app/docs/responses/replaygain/), [ratings](https://opensubsonic.netlify.app/docs/endpoints/setrating/), [scrobble](https://opensubsonic.netlify.app/docs/endpoints/scrobble/).

### Recommended Feasibility Experiments

1. Native playback proof on Windows, macOS, and Linux: representative gapless album files, mixed formats/rates, closed UI and keyboard control, USB interface disconnect, simultaneous large sync. Measure dropouts, transition continuity, CPU, memory, and sync throughput.
2. Streaming proof against actual Jellyfin/Navidrome instances: constrained bandwidth, server transcoding, seeking, buffer recovery, and mid-track quality changes. Report exactly which combinations preserve continuity.
3. Metadata proof on a representative collection: proportion of recordings with useful IDs, strong artist links, ReplayGain, and reliably matched duplicates across servers. Establish behavior when relationships are unavailable before committing to the full moving-center Radio.

Recommended initial architecture remains a daemon-owned playback subsystem, a shared selection core with separate Playback settings, and a detachable control UI. These experiments reduce risk without discarding any accepted product requirement.

### Explicit Platform Scope Correction

The user explicitly requires macOS and Linux as well as Windows. All three are playback targets from the start; the earlier Windows/macOS-only prototype wording was incomplete and is corrected above.

Use shared selection, queue/session state, streaming, and decoding with native audio and media-control adapters. Candidate targets are WASAPI shared on Windows, CoreAudio on macOS, and a shared desktop audio route on Linux. Verify the chosen crate version's Linux backend and coexistence with PipeWire/PulseAudio; do not assume direct ALSA hardware access satisfies shared-mode behavior.

Linux media integration should use MPRIS over the user's D-Bus session; Souvlaki is a candidate abstraction across all three OSes. Linux tray availability and event-loop integration vary by desktop and must be tested independently of playback and media keys. Include representative GNOME/KDE and Wayland/X11 testing, with exact supported distribution/package scope to be selected later. Existing release configuration and Linux smoke workflow are useful foundations, not proof of audio support.

On macOS the existing daemon uses an accessory activation policy and main-thread event loop. A menu-bar icon is distinct from a Dock menu: verify the user's intended entry surface and implement native integration deliberately. The main Tauri window must not be required for media control on any target.

Additional sources: [CPAL platform support](https://github.com/RustAudio/cpal), [Souvlaki](https://docs.rs/crate/souvlaki/latest), [tray-icon platform notes](https://github.com/tauri-apps/tray-icon).

## Feasibility Experiment Follow-Up

The user approved starting the proposed experiments. An isolated native Rust probe now exists in `experiments/playback-probe`, with measured results in `_bmad-output/implementation-artifacts/playback-feasibility-results.md` and implementation scope in `spec-playback-feasibility-probe.md` alongside it.

Mac synthetic decoding: WAV/FLAC/ALAC exact sample reproduction; MP3 passes fixture length/boundary checks; AAC adds padding; Opus unsupported in the tested decoder; Vorbis fixture unavailable with the installed encoder. Silent CoreAudio callbacks reported zero underruns, including synthetic I/O pressure. Basic stdin transport was exercised. Physical gapless output, real sync, OS media keys, Windows/Linux runtime, live-provider adaptation, and real-library relationship coverage remain pending.

Lifecycle inspection found UI-owned sidecars are killed on UI exit, especially relevant to Linux. Existing external user-session daemons survive. This needs a deliberate lifecycle change before the product can promise close-window/keep-listening behavior. The experiment does not alter production behavior or settle the final decoder choice.

### Decoder comparison follow-up

The approved isolated FFmpeg comparison passed WAV, FLAC, ALAC, MP3, AAC, and Opus on macOS with exact per-track lengths under the existing verifier. This resolves the synthetic AAC padding and Opus support obstacles for the CLI backend; it does not yet establish native Rust integration or Windows/Linux behavior. See `../implementation-artifacts/playback-feasibility-results.md` for measurements and the next integration decision.
