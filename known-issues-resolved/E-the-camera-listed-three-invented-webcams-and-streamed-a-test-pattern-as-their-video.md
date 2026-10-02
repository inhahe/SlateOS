### [E] The Camera listed three invented webcams and streamed a test pattern as their video -- 2026-09-27
**Status:** FIXED 2026-09-27 (lane E). A real build lists no camera and says
why; Take Photo and Record refuse without one.

**In short:** opening the Camera showed a Logitech C920, a Microsoft LifeCam
and a Razer Kiyo, all "Connected", with a moving picture in the viewfinder,
and Take Photo filed each "photo" in the gallery. None of it was real:
SlateOS has no video driver (the kernel's `fs::webcam` registry is filled
only from kshell, and `/proc/webcam` publishes counts), the devices were a
hard-coded list in `CameraApp::new`, and the picture was
`VideoFrame::new_test_pattern`. The 2026-09-15 fabrication sweep missed it;
lane E's sweep for "simulated" in production code found it.

**Now:** `new` lists no camera. The viewfinder says "No camera can be
reached." and why -- no video driver, so the list is empty for want of one,
not for want of a camera -- and draws nothing that could pass for a picture
(it had drawn the pattern under a "Camera error" band). A photo, a timer
countdown and a recording each refuse without a camera that is producing.
The invented devices are the tests' fixture (`with_sample_devices`, 57 call
sites moved); the capture pipeline they drive is the one a real device will.

**What would make it real:** a video-capture driver and a way for an
application to enumerate devices and read frames -- lane A's, and nothing
filed yet asks for it, because nothing yet needs it more than the camera
app itself.
