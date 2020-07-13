/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use num_bigint::BigUint;
use num_traits::{One, Zero};
use std::mem::replace;

/// Compute fibonacci numbers..
pub fn fibonacci(iterations: usize) -> BigUint {
  let mut current: BigUint = Zero::zero();
  let mut next: BigUint = One::one();

  for _ in 0..iterations {
    let new_next = current + &next;
    // This is a low cost way of swapping current with next and next with new_next.
    current = replace(&mut next, new_next);
  }

  current
}
