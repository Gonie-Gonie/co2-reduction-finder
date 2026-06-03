#![allow(dead_code)]

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Activation {
    Elu,
    Linear,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DenseLayer {
    pub weights: Vec<Vec<f32>>,
    pub bias: Vec<f32>,
    pub activation: Activation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MlpModel {
    pub input_dim: usize,
    pub output_dim: usize,
    pub layers: Vec<DenseLayer>,
}

impl MlpModel {
    pub fn predict(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        if input.len() != self.input_dim {
            return Err(format!(
                "invalid input dimension: expected {}, got {}",
                self.input_dim,
                input.len()
            ));
        }

        let mut values = input.to_vec();
        for layer in &self.layers {
            values = layer.forward(&values)?;
        }

        if values.len() != self.output_dim {
            return Err(format!(
                "invalid output dimension: expected {}, got {}",
                self.output_dim,
                values.len()
            ));
        }

        Ok(values)
    }

    pub fn predict_batch_parallel(&self, inputs: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, String> {
        inputs
            .par_iter()
            .map(|input| self.predict(input))
            .collect::<Result<Vec<_>, _>>()
    }

    pub fn parameter_count(&self) -> usize {
        self.layers
            .iter()
            .map(|layer| {
                let weight_count = layer.weights.iter().map(Vec::len).sum::<usize>();
                weight_count + layer.bias.len()
            })
            .sum()
    }
}

impl DenseLayer {
    fn forward(&self, input: &[f32]) -> Result<Vec<f32>, String> {
        if self.weights.len() != input.len() {
            return Err(format!(
                "invalid layer input dimension: expected {}, got {}",
                self.weights.len(),
                input.len()
            ));
        }

        let output_dim = self.bias.len();
        if self.weights.iter().any(|row| row.len() != output_dim) {
            return Err("dense layer weight matrix has inconsistent row lengths".to_string());
        }

        let mut output = self.bias.clone();
        for (input_value, weights_for_input) in input.iter().zip(&self.weights) {
            for (output_value, weight) in output.iter_mut().zip(weights_for_input) {
                *output_value += input_value * weight;
            }
        }

        for value in &mut output {
            *value = match self.activation {
                Activation::Elu => {
                    if *value >= 0.0 {
                        *value
                    } else {
                        value.exp() - 1.0
                    }
                }
                Activation::Linear => *value,
            };
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::{Activation, DenseLayer, MlpModel};

    #[test]
    fn predicts_dense_elu_linear_network() {
        let model = MlpModel {
            input_dim: 2,
            output_dim: 1,
            layers: vec![
                DenseLayer {
                    weights: vec![vec![1.0, -1.0], vec![0.5, 2.0]],
                    bias: vec![0.0, 0.0],
                    activation: Activation::Elu,
                },
                DenseLayer {
                    weights: vec![vec![2.0], vec![1.0]],
                    bias: vec![0.25],
                    activation: Activation::Linear,
                },
            ],
        };

        let predicted = model
            .predict(&[2.0, 1.0])
            .expect("prediction should succeed");
        assert!((predicted[0] - 5.25).abs() < 0.0001);
    }

    #[test]
    fn predicts_batch_in_parallel() {
        let model = MlpModel {
            input_dim: 1,
            output_dim: 1,
            layers: vec![DenseLayer {
                weights: vec![vec![2.0]],
                bias: vec![1.0],
                activation: Activation::Linear,
            }],
        };

        let predicted = model
            .predict_batch_parallel(&[vec![1.0], vec![2.0], vec![3.0]])
            .expect("batch prediction should succeed");

        assert_eq!(predicted, vec![vec![3.0], vec![5.0], vec![7.0]]);
    }
}
