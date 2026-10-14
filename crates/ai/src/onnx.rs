//! ONNX Runtime sessions. The graphics card where the platform offers it
//! (DirectML on Windows, Core ML on macOS), else the CPU; ONNX Runtime
//! falls back to the CPU by itself when the graphics card cannot run a
//! model.

use std::path::Path;
use std::sync::Mutex;

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;

/// A loaded model. ONNX Runtime needs exclusive access to run a session,
/// so calls take turns.
pub struct Model {
    session: Mutex<Session>,
    pub gpu: bool,
}

fn err<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

impl Model {
    pub fn load(path: &Path, gpu: bool) -> anyhow::Result<Self> {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
        let mut b = Session::builder().map_err(err)?.with_optimization_level(GraphOptimizationLevel::Level3).map_err(err)?;
        b = b.with_intra_threads(threads).map_err(err)?;
        let mut on_gpu = false;
        if gpu {
            #[cfg(windows)]
            {
                b = b.with_execution_providers([ort::ep::DirectML::default().build()]).map_err(err)?;
                on_gpu = true;
            }
            #[cfg(target_os = "macos")]
            {
                b = b.with_execution_providers([ort::ep::CoreML::default().build()]).map_err(err)?;
                on_gpu = true;
            }
        }
        let session = b.commit_from_file(path).map_err(err)?;
        Ok(Model { session: Mutex::new(session), gpu: on_gpu })
    }

    /// Runs the model on one input tensor and returns the first output as
    /// a flat vector with its shape.
    pub fn run_f32(&self, input: &str, shape: Vec<i64>, data: Vec<f32>) -> anyhow::Result<(Vec<i64>, Vec<f32>)> {
        let t = ort::value::Tensor::from_array((shape, data)).map_err(err)?;
        let mut s = self.session.lock().map_err(err)?;
        let out = s.run(ort::inputs![input => t]).map_err(err)?;
        let (shape, v) = out[0].try_extract_tensor::<f32>().map_err(err)?;
        Ok((shape.iter().copied().collect(), v.to_vec()))
    }

    pub fn run_i32(&self, input: &str, shape: Vec<i64>, data: Vec<i32>) -> anyhow::Result<(Vec<i64>, Vec<f32>)> {
        let t = ort::value::Tensor::from_array((shape, data)).map_err(err)?;
        let mut s = self.session.lock().map_err(err)?;
        let out = s.run(ort::inputs![input => t]).map_err(err)?;
        let (shape, v) = out[0].try_extract_tensor::<f32>().map_err(err)?;
        Ok((shape.iter().copied().collect(), v.to_vec()))
    }
}
