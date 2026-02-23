use std::cell::RefCell;

use rand::Rng;

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

    /// Apply LeakyReLU element-wise: max(0.01 * x, x).
    pub fn leaky_relu(&self) -> Tensor {
        let data: Vec<f32> = self.data.iter().map(|&x| if x > 0.0 { x } else { 0.01 * x }).collect();
        Tensor::new(data, self.requires_grad)
    }

    /// Apply tanh activation element-wise.
    pub fn tanh_act(&self) -> Tensor {
        let data: Vec<f32> = self.data.iter().map(|x| x.tanh()).collect();
        Tensor::new(data, self.requires_grad)
    }

    /// Apply ELU element-wise: x if x > 0, alpha * (exp(x) - 1) otherwise.
    pub fn elu(&self, alpha: f32) -> Tensor {
        let data: Vec<f32> = self
            .data
            .iter()
            .map(|&x| if x > 0.0 { x } else { alpha * (x.exp() - 1.0) })
            .collect();
        Tensor::new(data, self.requires_grad)
    }

    /// Element-wise subtraction.
    pub fn sub(&self, other: &Tensor) -> Tensor {
        assert_eq!(self.data.len(), other.data.len());
        let data: Vec<f32> = self.data.iter().zip(other.data.iter()).map(|(a, b)| a - b).collect();
        Tensor::new(data, self.requires_grad || other.requires_grad)
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

/// A 2D weight matrix stored in row-major order.
///
/// Used for learnable linear transformations in GNN layers (W_neigh, W_self)
/// and the classification head (W_out). Supports matrix-vector multiplication,
/// outer products for gradient accumulation, and Xavier initialization.
#[derive(Debug, Clone)]
pub struct Matrix {
    /// Row-major data: element at (i, j) is `data[i * cols + j]`.
    pub data: Vec<f32>,
    pub rows: usize,
    pub cols: usize,
}

impl Matrix {
    /// Create a zero-filled matrix.
    pub fn zeros(rows: usize, cols: usize) -> Self {
        Self {
            data: vec![0.0; rows * cols],
            rows,
            cols,
        }
    }

    /// Xavier/Glorot uniform initialization: U(-limit, limit)
    /// where limit = sqrt(6 / (fan_in + fan_out)).
    pub fn random_xavier(rows: usize, cols: usize, rng: &mut impl Rng) -> Self {
        let limit = (6.0f32 / (rows + cols) as f32).sqrt();
        let data: Vec<f32> = (0..rows * cols)
            .map(|_| rng.r#gen::<f32>() * 2.0 * limit - limit)
            .collect();
        Self { data, rows, cols }
    }

    /// Matrix-vector multiply: self [rows x cols] * v [cols] -> result [rows].
    ///
    /// # Panics
    /// Panics if `v.len() != self.cols`.
    pub fn matvec(&self, v: &Tensor) -> Tensor {
        assert_eq!(
            v.data.len(),
            self.cols,
            "Matrix::matvec: dimension mismatch ({} cols vs {} vec len)",
            self.cols,
            v.data.len()
        );
        let mut result = vec![0.0; self.rows];
        for i in 0..self.rows {
            let row_start = i * self.cols;
            let mut sum = 0.0;
            for j in 0..self.cols {
                sum += self.data[row_start + j] * v.data[j];
            }
            result[i] = sum;
        }
        Tensor::new(result, false)
    }

    /// Transposed matrix-vector multiply: self^T [cols x rows] * v [rows] -> result [cols].
    ///
    /// Equivalent to `self.transpose().matvec(v)` but without allocating the transpose.
    ///
    /// # Panics
    /// Panics if `v.len() != self.rows`.
    pub fn transpose_matvec(&self, v: &Tensor) -> Tensor {
        assert_eq!(
            v.data.len(),
            self.rows,
            "Matrix::transpose_matvec: dimension mismatch ({} rows vs {} vec len)",
            self.rows,
            v.data.len()
        );
        let mut result = vec![0.0; self.cols];
        for i in 0..self.rows {
            let row_start = i * self.cols;
            let vi = v.data[i];
            for j in 0..self.cols {
                result[j] += self.data[row_start + j] * vi;
            }
        }
        Tensor::new(result, false)
    }

    /// Outer product: a [rows] x b [cols] -> Matrix [rows x cols].
    ///
    /// Used for accumulating weight gradients: dW += dL/dz * h^T.
    pub fn outer(a: &Tensor, b: &Tensor) -> Self {
        let rows = a.data.len();
        let cols = b.data.len();
        let mut data = vec![0.0; rows * cols];
        for i in 0..rows {
            let row_start = i * cols;
            for j in 0..cols {
                data[row_start + j] = a.data[i] * b.data[j];
            }
        }
        Self { data, rows, cols }
    }

    /// Element-wise addition. Both matrices must have the same shape.
    ///
    /// # Panics
    /// Panics if shapes differ.
    pub fn add(&self, other: &Matrix) -> Matrix {
        assert_eq!(self.rows, other.rows, "Matrix::add: row mismatch");
        assert_eq!(self.cols, other.cols, "Matrix::add: col mismatch");
        let data: Vec<f32> = self
            .data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| a + b)
            .collect();
        Matrix {
            data,
            rows: self.rows,
            cols: self.cols,
        }
    }

    /// Scalar multiplication.
    pub fn scale(&self, s: f32) -> Matrix {
        let data: Vec<f32> = self.data.iter().map(|x| x * s).collect();
        Matrix {
            data,
            rows: self.rows,
            cols: self.cols,
        }
    }

    /// Transpose: [rows x cols] -> [cols x rows].
    pub fn transpose(&self) -> Matrix {
        let mut data = vec![0.0; self.rows * self.cols];
        for i in 0..self.rows {
            for j in 0..self.cols {
                data[j * self.rows + i] = self.data[i * self.cols + j];
            }
        }
        Matrix {
            data,
            rows: self.cols,
            cols: self.rows,
        }
    }

    /// In-place subtraction: self -= other. Used for SGD: w -= lr * grad.
    ///
    /// # Panics
    /// Panics if shapes differ.
    pub fn sub_assign(&mut self, other: &Matrix) {
        assert_eq!(self.rows, other.rows, "Matrix::sub_assign: row mismatch");
        assert_eq!(self.cols, other.cols, "Matrix::sub_assign: col mismatch");
        for (a, b) in self.data.iter_mut().zip(other.data.iter()) {
            *a -= *b;
        }
    }

    /// Zero out all elements.
    pub fn zero(&mut self) {
        self.data.fill(0.0);
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

    #[test]
    fn test_matrix_matvec() {
        // [[1, 2], [3, 4]] * [5, 6] = [1*5+2*6, 3*5+4*6] = [17, 39]
        let m = Matrix {
            data: vec![1.0, 2.0, 3.0, 4.0],
            rows: 2,
            cols: 2,
        };
        let v = Tensor::new(vec![5.0, 6.0], false);
        let result = m.matvec(&v);
        assert!((result.data[0] - 17.0).abs() < 1e-6);
        assert!((result.data[1] - 39.0).abs() < 1e-6);
    }

    #[test]
    fn test_matrix_transpose_matvec() {
        // [[1, 2], [3, 4]]^T * [5, 6] = [[1,3],[2,4]] * [5,6] = [1*5+3*6, 2*5+4*6] = [23, 34]
        let m = Matrix {
            data: vec![1.0, 2.0, 3.0, 4.0],
            rows: 2,
            cols: 2,
        };
        let v = Tensor::new(vec![5.0, 6.0], false);
        let result = m.transpose_matvec(&v);
        assert!((result.data[0] - 23.0).abs() < 1e-6);
        assert!((result.data[1] - 34.0).abs() < 1e-6);
    }

    #[test]
    fn test_matrix_non_square_matvec() {
        // [[1, 2, 3], [4, 5, 6]] * [1, 2, 3] = [1+4+9, 4+10+18] = [14, 32]
        let m = Matrix {
            data: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            rows: 2,
            cols: 3,
        };
        let v = Tensor::new(vec![1.0, 2.0, 3.0], false);
        let result = m.matvec(&v);
        assert_eq!(result.data.len(), 2);
        assert!((result.data[0] - 14.0).abs() < 1e-6);
        assert!((result.data[1] - 32.0).abs() < 1e-6);
    }

    #[test]
    fn test_matrix_xavier_init() {
        let mut rng = rand::thread_rng();
        let m = Matrix::random_xavier(64, 32, &mut rng);
        assert_eq!(m.rows, 64);
        assert_eq!(m.cols, 32);
        assert_eq!(m.data.len(), 64 * 32);
        let limit = (6.0f32 / (64 + 32) as f32).sqrt();
        for &val in &m.data {
            assert!(val >= -limit && val <= limit);
        }
    }

    #[test]
    fn test_matrix_outer() {
        let a = Tensor::new(vec![1.0, 2.0], false);
        let b = Tensor::new(vec![3.0, 4.0, 5.0], false);
        let m = Matrix::outer(&a, &b);
        assert_eq!(m.rows, 2);
        assert_eq!(m.cols, 3);
        // [[1*3, 1*4, 1*5], [2*3, 2*4, 2*5]] = [[3,4,5],[6,8,10]]
        assert!((m.data[0] - 3.0).abs() < 1e-6);
        assert!((m.data[1] - 4.0).abs() < 1e-6);
        assert!((m.data[2] - 5.0).abs() < 1e-6);
        assert!((m.data[3] - 6.0).abs() < 1e-6);
        assert!((m.data[4] - 8.0).abs() < 1e-6);
        assert!((m.data[5] - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_matrix_transpose() {
        // [[1, 2, 3], [4, 5, 6]] -> [[1, 4], [2, 5], [3, 6]]
        let m = Matrix {
            data: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            rows: 2,
            cols: 3,
        };
        let t = m.transpose();
        assert_eq!(t.rows, 3);
        assert_eq!(t.cols, 2);
        assert_eq!(t.data, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn test_matrix_add_scale() {
        let a = Matrix {
            data: vec![1.0, 2.0, 3.0, 4.0],
            rows: 2,
            cols: 2,
        };
        let b = Matrix {
            data: vec![5.0, 6.0, 7.0, 8.0],
            rows: 2,
            cols: 2,
        };
        let c = a.add(&b);
        assert_eq!(c.data, vec![6.0, 8.0, 10.0, 12.0]);

        let d = a.scale(2.0);
        assert_eq!(d.data, vec![2.0, 4.0, 6.0, 8.0]);
    }

    #[test]
    fn test_matrix_sub_assign() {
        let mut a = Matrix {
            data: vec![10.0, 20.0, 30.0, 40.0],
            rows: 2,
            cols: 2,
        };
        let b = Matrix {
            data: vec![1.0, 2.0, 3.0, 4.0],
            rows: 2,
            cols: 2,
        };
        a.sub_assign(&b);
        assert_eq!(a.data, vec![9.0, 18.0, 27.0, 36.0]);
    }
}
