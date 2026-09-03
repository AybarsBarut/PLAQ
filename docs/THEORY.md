# Theory and research hypothesis

Digitizing a groove or stylus trajectory does not create information and does
not evade sampling or quantization. PLAQ starts with signed PCM and merely tests
a different reversible coordinate system:

> Correlated stereo PCM may yield lower-entropy residuals after integer
> mid/side lifting and block-local position-, velocity-, or acceleration-like
> prediction.

This is falsifiable. For a block component `x[n]`, PLAQ compares raw values,
first differences, linear second-order prediction, and cubic third-order
prediction. It then measures the exact Rice code length for every `k` from 0 to
31. A signal with smooth local motion can produce small residuals; white noise,
hard clipping, decorrelated channels, and predictor resets can erase the benefit.

The mid/side lifting is bijective over the supported integer range. No floating
operation, resampling, quantization, or sample removal appears in the lossless
path. CRC32C detects damaged compressed blocks and SHA-256 verifies the complete
reconstructed PCM sequence.

## Physical simulation

`plaq simulate` is a separate listening experiment. It integrates a damped
mass/spring follower with configurable mass, compliance, damping, displacement,
velocity, and tracking-error terms. It normalizes the virtual axes, uses `f64`,
clips state, rounds back to PCM, and is necessarily lossy. It does not write a
lossless-profile `.plaq` file and is never called during `encode` or `decode`.

