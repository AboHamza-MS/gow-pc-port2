//! Developer tools of the port, in one binary (`ps2-dev`): config and test validators for CI, summaries of the profiles
//! and inspectors of local captures. None of them needs the game's files to run its tests.

pub mod calls;
pub mod captures;
pub mod checks;
pub mod perf;
pub mod rate;
pub mod sampling;
pub mod util;
pub mod validate;
