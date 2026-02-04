use std::cell::RefCell;

/// A 1D differentiable tensor with gradient tracking.
///
/// This is a minimal tensor type for GNN feature vectors. It supports
/// element-wise operations, activations, and gradient storage for
/// backpropagation-style weight updates.
#[derive(Clone)]
pub struct Tensor {
    pub data: Vec<f32>,
    pub grad: RefCell<Option<Vec<f32>>>,
    pub requires_grad: bool,
}

impl Tensor {
    /// Create a new tensor from the given data.
    pub fn new(data: Vec<f32>, requires_grad: bool) -> Self {
        Self {
            data,
            grad: RefCell::new(None),
            requires_grad,
        }
    }

    /// Create a zero-filled tensor of the given length.
    pub fn zeros(len: usize, requires_grad: bool) -> Self {
        Self::new(vec![0.0; len], requires_grad)
    }

    /// Create a tensor containing a single scalar value.
    pub fn from_scalar(value: f32) -> Self {
        Self::new(vec![value], false)
    }

    /// Number of elements in the tensor.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the tensor is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Element-wise addition. Both tensors must have the same length.
    ///
    /// # Panics
    /// Panics if the tensors have different lengths.
    pub fn add(&self, other: &Tensor) -> Tensor {
        assert_eq!(
            self.data.len(),
            other.data.len(),
            "Tensor::add: length mismatch ({} vs {})",
            self.data.len(),
            other.data.len()
        );
        let data: Vec<f32> = self
            .data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| a + b)
            .collect();
        Tensor::new(data, self.requires_grad || other.requires_grad)
    }

    /// Element-wise multiplication (Hadamard product). Both tensors must have the same length.
    ///
    /// # Panics
    /// Panics if the tensors have different lengths.
    pub fn mul(&self, other: &Tensor) -> Tensor {
        assert_eq!(
            self.data.len(),
            other.data.len(),
            "Tensor::mul: length mismatch ({} vs {})",
            self.data.len(),
            other.data.len()
        );
        let data: Vec<f32> = self
            .data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| a * b)
            .collect();
        Tensor::new(data, self.requires_grad || other.requires_grad)
    }

    /// Scalar multiplication: multiply every element by `s`.
    pub fn scale(&self, s: f32) -> Tensor {
        let data: Vec<f32> = self.data.iter().map(|x| x * s).collect();
        Tensor::new(data, self.requires_grad)
    }

    /// Dot product with another tensor. Both must have the same length.
    ///
    /// # Panics
    /// Panics if the tensors have different lengths.
    pub fn dot(&self, other: &Tensor) -> f32 {
        assert_eq!(
            self.data.len(),
            other.data.len(),
            "Tensor::dot: length mismatch ({} vs {})",
            self.data.len(),
            other.data.len()
        );
        self.data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| a * b)
            .sum()
    }

    /// Sum all elements.
    pub fn sum(&self) -> f32 {
        self.data.iter().sum()
    }

    /// Apply ReLU activation element-wise: max(0, x).
    pub fn relu(&self) -> Tensor {
        let data: Vec<f32> = self.data.iter().map(|x| x.max(0.0)).collect();
        Tensor::new(data, self.requires_grad)
    }

    /// Apply sigmoid activation element-wise: 1 / (1 + exp(-x)).
    pub fn sigmoid(&self) -> Tensor {
        let data: Vec<f32> = self
            .data
            .iter()
            .map(|x| 1.0 / (1.0 + (-x).exp()))
            .collect();
        Tensor::new(data, self.requires_grad)
    }

    /// L2 norm: sqrt(sum(x_i^2)).
    pub fn norm(&self) -> f32 {
        let sum_sq: f32 = self.data.iter().map(|x| x * x).sum();
        sum_sq.sqrt()
    }

    /// Mean of all elements.
    pub fn mean(&self) -> f32 {
        if self.data.is_empty() {
            return 0.0;
        }
        self.sum() / self.data.len() as f32
    }

    /// Set the gradient for this tensor.
    pub fn set_grad(&self, grad: Vec<f32>) {
        *self.grad.borrow_mut() = Some(grad);
    }

    /// Get the gradient (clone).
    pub fn grad(&self) -> Option<Vec<f32>> {
        self.grad.borrow().clone()
    }

    /// Zero out the gradient.
    pub fn zero_grad(&self) {
        *self.grad.borrow_mut() = None;
    }
}

impl std::fmt::Debug for Tensor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tensor")
            .field("data", &self.data)
            .field("requires_grad", &self.requires_grad)
            .field("has_grad", &self.grad.borrow().is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tensor_add() {
        let a = Tensor::new(vec![1.0, 2.0, 3.0], false);
        let b = Tensor::new(vec![4.0, 5.0, 6.0], false);
        let c = a.add(&b);
        assert_eq!(c.data, vec![5.0, 7.0, 9.0]);
    }

    #[test]
    fn test_tensor_mul() {
        let a = Tensor::new(vec![2.0, 3.0, 4.0], false);
        let b = Tensor::new(vec![5.0, 6.0, 7.0], false);
        let c = a.mul(&b);
        assert_eq!(c.data, vec![10.0, 18.0, 28.0]);
    }

    #[test]
    fn test_tensor_scale() {
        let a = Tensor::new(vec![1.0, 2.0, 3.0], false);
        let b = a.scale(3.0);
        assert_eq!(b.data, vec![3.0, 6.0, 9.0]);
    }

    #[test]
    fn test_tensor_dot() {
        let a = Tensor::new(vec![1.0, 2.0, 3.0], false);
        let b = Tensor::new(vec![4.0, 5.0, 6.0], false);
        let result = a.dot(&b);
        // 1*4 + 2*5 + 3*6 = 4 + 10 + 18 = 32
        assert!((result - 32.0).abs() < 1e-6);
    }

    #[test]
    fn test_tensor_relu() {
        let a = Tensor::new(vec![-2.0, -1.0, 0.0, 1.0, 2.0], false);
        let b = a.relu();
        assert_eq!(b.data, vec![0.0, 0.0, 0.0, 1.0, 2.0]);
    }

    #[test]
    fn test_tensor_sigmoid() {
        let a = Tensor::new(vec![-10.0, 0.0, 10.0], false);
        let b = a.sigmoid();
        // sigmoid(-10) ~ 0.0000454, sigmoid(0) = 0.5, sigmoid(10) ~ 0.9999546
        assert!(b.data[0] < 0.001);
        assert!((b.data[1] - 0.5).abs() < 1e-6);
        assert!(b.data[2] > 0.999);

        // All sigmoid outputs must be in (0, 1).
        for val in &b.data {
            assert!(*val > 0.0 && *val < 1.0);
        }
    }

    #[test]
    fn test_tensor_norm() {
        let a = Tensor::new(vec![3.0, 4.0], false);
        let n = a.norm();
        // sqrt(9 + 16) = sqrt(25) = 5
        assert!((n - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_tensor_grad() {
        let a = Tensor::new(vec![1.0, 2.0], true);

        // Initially no gradient.
        assert!(a.grad().is_none());

        // Set gradient.
        a.set_grad(vec![0.5, 0.5]);
        let g = a.grad().unwrap();
        assert_eq!(g, vec![0.5, 0.5]);

        // Zero grad.
        a.zero_grad();
        assert!(a.grad().is_none());
    }

    #[test]
    fn test_tensor_zeros() {
        let t = Tensor::zeros(5, false);
        assert_eq!(t.len(), 5);
        assert_eq!(t.data, vec![0.0; 5]);
    }

    #[test]
    fn test_tensor_from_scalar() {
        let t = Tensor::from_scalar(3.14);
        assert_eq!(t.len(), 1);
        assert!((t.data[0] - 3.14).abs() < 1e-6);
    }

    #[test]
    fn test_tensor_sum() {
        let t = Tensor::new(vec![1.0, 2.0, 3.0, 4.0], false);
        assert!((t.sum() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_tensor_mean() {
        let t = Tensor::new(vec![2.0, 4.0, 6.0], false);
        assert!((t.mean() - 4.0).abs() < 1e-6);
    }

    #[test]
    fn test_tensor_empty_mean() {
        let t = Tensor::new(vec![], false);
        assert!(t.is_empty());
        assert!((t.mean() - 0.0).abs() < 1e-6);
    }
}
