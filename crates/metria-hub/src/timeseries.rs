//! 趋势图共享策略：分桶与确定性降采样。

pub const MAX_CHART_POINTS: usize = 240;

pub fn downsample_indices(len: usize, max_points: usize) -> Vec<usize> {
    if len <= max_points {
        return (0..len).collect();
    }
    let step = len.div_ceil(max_points);
    (0..len).step_by(step).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsampling_matches_chart_point_limit() {
        assert_eq!(downsample_indices(3, MAX_CHART_POINTS), vec![0, 1, 2]);
        assert_eq!(downsample_indices(336, MAX_CHART_POINTS).len(), 168);
        assert_eq!(downsample_indices(336, MAX_CHART_POINTS)[..3], [0, 2, 4]);
        assert_eq!(downsample_indices(337, MAX_CHART_POINTS).len(), 169);
    }
}
