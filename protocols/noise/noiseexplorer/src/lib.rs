/*
IK:
  <- s
  ...
  -> e, es, s, ss
  <- e, ee, se
  ->
  <-
*/

/* ---------------------------------------------------------------- *
 * PARAMETERS                                                       *
 * ---------------------------------------------------------------- */

#[macro_use]
pub(crate) mod macros;

pub(crate) mod prims;
pub(crate) mod state_ik;
pub(crate) mod state_ix;
pub(crate) mod state_xx;
pub(crate) mod utils;

pub mod consts;
pub mod error;
pub mod noisesession_ik;
pub mod noisesession_ix;
pub mod noisesession_xx;
pub mod types;