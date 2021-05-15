// Symphonia
// Copyright (c) 2021 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `mdct` module implements the Modified Discrete Cosine Transform (MDCT).
//!
//! The (I)MDCT algorithms in this module are not general purpose and are specialized for use in
//! typical audio compression applications. Therefore, some constraints may apply.

use std::f64;

use super::dct::Dct;

/// Inverse Modified Discrete Transform (IMDCT).
///
/// Implements the IMDCT in-terms of a DCT-IV as described in \[1\] and \[2\].
///
/// \[1\] Mu-Huo Cheng and Yu-Hsin Hsu, "Fast IMDCT and MDCT algorithms - a matrix approach,"
///       in IEEE Transactions on Signal Processing, vol. 51, no. 1, pp. 221-229, Jan. 2003,
///       doi: 10.1109/TSP.2002.806566.
///
/// \[2\] Tan Li, R. Zhang, R. Yang, Heyun Huang and Fuhuei Lin, "A unified computing kernel for
///       MDCT/IMDCT in modern audio coding standards," 2007 International Symposium on
///       Communications and Information Technologies, Sydney, NSW, 2007, pp. 546-550,
///       doi: 10.1109/ISCIT.2007.4392079.
pub struct Imdct {
    dct: Dct,
    n: u32,
    table: Vec<f32>,
}

impl Imdct {
    /// Instantiate a N-point IMDCT.
    ///
    /// The value of `n` must be a power-of-2, and less-than or equal to 8192.
    pub fn new(n: u32) -> Imdct {
        // The algorithm implemented requires a power-of-two N.
        assert!(n.is_power_of_two(), "n must be a power of two");
        // This limitation is somewhat arbitrary, but a limit must be set somewhere.
        assert!(n <= 8192, "maximum of 8192-point imdct");

        let c = f64::consts::PI / f64::from(2 * 2 * n);

        let table: Vec<f32> = (0..n)
            .map(|i| (2.0 * (c * f64::from(2 * i + 1)).cos()) as f32)
            .collect();

        Imdct {
            dct: Dct::new(n),
            n,
            table,
        }
    }

    /// Performs the the N-point Inverse Modified Discrete Cosine Transform.
    ///
    /// The number of input samples in `src`, N, must equal the value `Imdct` was instantiated with.
    /// The length of the output slice, `dst`, must equal 2N. Failing to meet these requirements
    /// will throw an assertion.
    ///
    /// This function performs no windowing, but each sample will be multiplied by `scale`. Typically,
    /// scale will equal `sqrt(1.0 / N)` where N is the number of input samples, though each
    /// application will vary.
    pub fn imdct(&mut self, src: &[f32], dst: &mut [f32], scale: f32) {
        // The IMDCT produces 2N samples for N inputs. This algorithm defines the ouput length as
        // N.
        let n2 = self.n as usize;
        let n = n2 << 1;
        let n4 = n2 >> 1;

        assert_eq!(dst.len(), n);
        assert_eq!(src.len(), n2);

        // Pre-process the input and place it in the second-half of dst.
        for ((ds, &src), &cos) in dst[n2..].iter_mut().zip(src).zip(&self.table) {
            *ds = src * cos;
        }

        // Compute the DCT-II in-place using the pre-processed samples that reside in the second-
        // half of dst.
        self.dct.dct_ii_inplace(&mut dst[n2..]);

        // DCT-II to DCT-IV
        //
        // Split dst into 4 evenly sized N/4 regions: [ a, b, c, d ]. Regions c & d contain the
        // DCT-II transformed samples from the previous step. After this step, regions b & c will
        // contain the DCT-II transformed samples.
        let (a, b) = dst.split_at_mut(n4);
        let (b, c) = b.split_at_mut(n4);
        let (c, d) = c.split_at_mut(n4);

        // Map c to b.
        b[0] = -0.5 * c[0];

        for i in 1..n4 {
            b[i] = -1.0 * (c[i] + b[i - 1]);
        }

        // Map d to c.
        c[0] = d[0] + b[n4 - 1];

        for i in 1..n4 {
            c[i] = d[i] - c[i - 1];
        }

        // DCT-IV to IMDCT
        //
        // Using symmetry, expand the DCT-IV to IMDCT. Multiply by the scale factor as this
        // is done.
        for (sa, &sc) in a.iter_mut().zip(c.iter()) {
            // Region a is a scaled copy of region c.
            *sa = scale * sc;
        }

        for ((sd, sc), &sb) in d.iter_mut().zip(c.iter_mut().rev()).zip(b.iter()) {
            // Region d is a scaled copy of region b.
            // Region c is a reversed and scaled copy of region b.
            let s = scale * sb;
            *sd = s;
            *sc = s;
        }

        for (sb, &sa) in b.iter_mut().zip(a.iter().rev()) {
            // Region b is an inverted copy of region c. Region c was overwrittern above,
            // but region a is a copy of the original region c.
            *sb = -1.0 * sa;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64;

    fn imdct_analytical(x: &[f32], y: &mut [f32], scale: f64) {
        assert!(y.len() == 2 * x.len());

        // Generates 2N outputs from N inputs.
        let n_in = x.len();
        let n_out = x.len() << 1;

        let pi_2n = f64::consts::PI / (2 * n_out) as f64;

        for i in 0..n_out {
            let mut accum = 0.0;

            for j in 0..n_in {
                accum +=
                    f64::from(x[j]) * (pi_2n * ((2 * i + 1 + n_in) * (2 * j + 1)) as f64).cos();
            }

            y[i] = (scale * accum) as f32;
        }
    }

    #[test]
    fn verify_imdct() {
        const TEST_VECTOR: [f32; 32] = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
            17.0, 18.0, 19.0, 20.0, 21.0, 22.0, 23.0, 24.0, 25.0, 26.0, 27.0, 28.0, 29.0, 30.0,
            31.0, 32.0,
        ];

        let mut actual = [0f32; 64];
        let mut expected = [0f32; 64];

        let scale = (2.0f64 / 64.0).sqrt();

        imdct_analytical(&TEST_VECTOR, &mut expected, scale);

        let mut mdct = Imdct::new(32);
        mdct.imdct(&TEST_VECTOR, &mut actual, scale as f32);

        for i in 0..64 {
            assert!((actual[i] - expected[i]).abs() < 0.00001);
        }
    }
}
