# Synced playback

## Goal

Play all shown cameras at the same time of day. Show black where a camera has no video. Change from one chunk to the next with no visible stop.

## Depends on

- [folder-scan-and-timeline.md](folder-scan-and-timeline.md) for the library and the `find(camera, time)` lookup.

## The clock

- There is one clock for the whole app. Its value is a time inside the total time range.
- While playing, the clock moves forward by `real time passed x speed`.
- The clock is the only source of time. Cameras follow the clock. The clock never waits for a camera.
- The clock stops and playback pauses when it reaches the end of the total time range.

## What each tile shows

For each shown camera, on each screen refresh:

```
result = find(camera, clock time)
if result is a chunk:
  position = clock time - chunk start
  show the frame of that chunk at that position
else:
  show black
```

- A frame is the correct frame when its position is the latest one that is not after the clock position.
- If decoding is too slow, skip frames to catch up. Never let a camera fall behind the clock.
- If a frame is not ready yet (for example right after a jump), show black until it is ready. Do not show a frame from a different time.

## Gaps

- In a gap, the tile is black. This includes the time before the first chunk and after the last chunk of a camera.
- Exception: if a gap is shorter than 0.5 seconds, keep the last frame of the earlier chunk on screen. This prevents a black flash.
- Other cameras keep playing during the gap of one camera.

## Smooth change between chunks

- Open the next chunk of each camera ahead of time, at least 5 seconds of real time before the clock reaches its start.
- Decode its first frame ahead of time, so the frame is on screen when the clock reaches the chunk start.
- Do the same when a gap ends.
- Opening and decoding must not block the screen refresh or the other cameras.
- Close a chunk after the clock leaves it.

## Jump

- A jump sets the clock to a new time. See [controls-and-layout.md](controls-and-layout.md) for how the user starts a jump.
- After a jump, each camera does the lookup again and opens the correct chunk at the correct position.
- Each camera must show the exact frame for the new time, not only the nearest key frame (a frame that can be decoded without earlier frames).
- The play or pause state stays the same after a jump.
- While paused, a jump still updates all tiles to the new time.

## Speed

- Speed changes the rate of the clock only. All cameras stay in sync because they follow the clock.
- At high speed the app can skip frames. It must not slow the clock.
- From the speed of the "Key frames only from" setting (default 4x), a camera shows key frames only. This rule applies to a video only if its key frames come at least 2 times each real second at the current speed. In other cases the camera decodes all frames. See "Settings" in [controls-and-layout.md](controls-and-layout.md).
- The 5 second rule for opening the next chunk uses real time, so it starts earlier on the timeline at high speed.

## Hidden cameras

- Do not open or decode chunks for a hidden camera.
- When a camera becomes shown again, it joins at the current clock time.

## Edge cases and errors

- A chunk fails to open or decode during playback: show black for that camera until its next chunk. Do not stop the other cameras.
- A chunk is shorter than its stored length: show black from the last frame to the chunk end.
- The computer cannot decode all cameras at full rate: skip frames. Keep the clock at the correct rate.

## Done when

- With 4 cameras playing at 1x, the times of the frames on screen differ by no more than 100 ms between cameras.
- After 30 minutes of play at 1x, the clock has moved 30 minutes, within 1 second.
- A camera with a missing chunk shows black for that time range while the other cameras play.
- A change from one chunk to the next shows no freeze longer than 100 ms, apart from the real gap.
- After a jump, all tiles show the correct time within 1 second of real time.
- The app plays no sound.
