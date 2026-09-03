# Known limitations

- PLAQ is an experimental research format, not a production archival standard.
- Only integer PCM WAV, mono/stereo, 16-bit/24-bit, up to 768 kHz is accepted.
- RIFF metadata and chunk ordering are not preserved; required audio metadata
  and canonical PCM samples are preserved.
- Encode and decode currently buffer complete inputs, with explicit safety caps;
  block APIs exist internally but true constant-memory file streaming does not.
- Predictor search is exhaustive and favors clarity over encode speed.
- Rice coding alone is often weaker than mature context and entropy models.
- The synthetic benchmark cannot predict behavior on a music collection.
- UDP has reordering and detection but no retransmission or FEC. One lost
  fragment makes the receive fail explicitly.
- TCP/UDP demos have no encryption, authentication, or congestion control.
- The physical simulation omits RIAA, groove geometry, surface noise,
  wow/flutter, and tracing distortion. It is lossy and not a mastering model.
- Fuzz targets are supplied, but long-duration continuous fuzzing is not part of
  ordinary CI.

These constraints are deliberate disclosures. None supports claims such as
"infinite quality", recovery beyond the source PCM, or guaranteed superiority
to FLAC.

