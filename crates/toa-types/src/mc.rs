//! The kind `mc` of evidence record (`spec/types/en/evd.md`): the file `evidence/samples.csv`
//! and the statistics of `metrics`, computed from it alike everywhere.

/// Where `samples.csv` is in a record.
pub const SAMPLES: &str = "evidence/samples.csv";

/// The content of `samples.csv`: the full metric names of its columns, and for each sample its
/// number and the value of each metric, `None` when the sample did not give it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Samples {
    pub metrics: Vec<String>,
    pub rows: Vec<(i64, Vec<Option<f64>>)>,
}

/// A number as `samples.csv` writes it: a decimal number that reads back as the same number
/// (the shortest such form).
pub fn number_text(x: f64) -> String {
    format!("{x:?}")
}

impl Samples {
    /// The text of the file: UTF-8, lines ending in LF.
    pub fn text(&self) -> String {
        let mut s = String::from("sample");
        for m in &self.metrics {
            s.push(',');
            s.push_str(m);
        }
        s.push('\n');
        for (k, vals) in &self.rows {
            s.push_str(&k.to_string());
            for v in vals {
                s.push(',');
                if let Some(x) = v {
                    s.push_str(&number_text(*x));
                }
            }
            s.push('\n');
        }
        s
    }

    /// Read the file; `Err` says what is wrong with it.
    pub fn parse(bytes: &[u8]) -> Result<Samples, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8".to_string())?;
        if text.contains('\r') {
            return Err("its lines end in CR LF; they end in LF".into());
        }
        if !text.ends_with('\n') {
            return Err("its last line does not end in LF".into());
        }
        let mut lines = text[..text.len() - 1].split('\n');
        let head = lines.next().unwrap_or("");
        let mut cols = head.split(',');
        if cols.next() != Some("sample") {
            return Err("its first line starts with sample".into());
        }
        let metrics: Vec<String> = cols.map(str::to_string).collect();
        if metrics.iter().any(|m| !crate::v::full_metric(m)) {
            return Err("the first line holds sample and full metric names <testbench name>.<id>".into());
        }
        let mut rows = Vec::new();
        for (i, l) in lines.enumerate() {
            let line = i + 2;
            let mut cells = l.split(',');
            let ks = cells.next().unwrap_or("");
            let k: i64 = ks
                .parse()
                .ok()
                .filter(|k: &i64| *k >= 1 && k.to_string() == ks)
                .ok_or(format!("line {line}: the number of a sample, 1 or more, comes first"))?;
            let vals: Vec<&str> = cells.collect();
            if vals.len() != metrics.len() {
                return Err(format!("line {line}: {} values, for {} metrics", vals.len(), metrics.len()));
            }
            let mut row = Vec::with_capacity(vals.len());
            for v in vals {
                if v.is_empty() {
                    row.push(None);
                    continue;
                }
                let ok = v.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'));
                match v.parse::<f64>() {
                    Ok(x) if ok && x.is_finite() => row.push(Some(x)),
                    _ => return Err(format!("line {line}: {v:?} is not a decimal number")),
                }
            }
            rows.push((k, row));
        }
        Ok(Samples { metrics, rows })
    }

    /// The values of one column.
    pub fn column(&self, metric: &str) -> Option<Vec<Option<f64>>> {
        let i = self.metrics.iter().position(|m| m == metric)?;
        Some(self.rows.iter().map(|(_, r)| r[i]).collect())
    }
}

/// The statistics of a metric over the samples (`evd`, the kind `mc`).
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub n: u64,
    pub failed: u64,
    pub mean: Option<f64>,
    /// The standard deviation with `n − 1` as divisor; when `n` is 2 or more.
    pub sigma: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// The statistics of the values of a column, in the order of the samples: the mean as the sum
/// in that order divided by `n`, sigma from the sum of the squared differences from it.
pub fn stats(values: &[Option<f64>]) -> Stats {
    let xs: Vec<f64> = values.iter().flatten().copied().collect();
    let n = xs.len() as u64;
    let failed = values.len() as u64 - n;
    if xs.is_empty() {
        return Stats { n, failed, mean: None, sigma: None, min: None, max: None };
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let sigma = (xs.len() >= 2)
        .then(|| (xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (xs.len() - 1) as f64).sqrt());
    let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Stats { n, failed, mean: Some(mean), sigma, min: Some(min), max: Some(max) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_read_back() {
        let s = Samples {
            metrics: vec!["tb_a.x".into(), "tb_a.y".into()],
            rows: vec![
                (1, vec![Some(4.767109420596355e-11), None]),
                (2, vec![None, None]),
                (3, vec![Some(-0.1), Some(1.0)]),
            ],
        };
        let t = s.text();
        assert_eq!(t, "sample,tb_a.x,tb_a.y\n1,4.767109420596355e-11,\n2,,\n3,-0.1,1.0\n");
        assert_eq!(Samples::parse(t.as_bytes()).unwrap(), s);
        for bad in [
            "sample,tb_a.x\r\n1,2\r\n",
            "sample,tb_a.x\n1,2",
            "samples,tb_a.x\n1,2\n",
            "sample,x\n1,2\n",
            "sample,tb_a.x\n0,2\n",
            "sample,tb_a.x\n1,2,3\n",
            "sample,tb_a.x\n1,inf\n",
            "sample,tb_a.x\n1,0x10\n",
        ] {
            assert!(Samples::parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_statistics() {
        let s = stats(&[Some(1.0), None, Some(3.0), Some(2.0)]);
        assert_eq!(s, Stats { n: 3, failed: 1, mean: Some(2.0), sigma: Some(1.0), min: Some(1.0), max: Some(3.0) });
        let one = stats(&[Some(0.5)]);
        assert_eq!((one.n, one.sigma, one.mean), (1, None, Some(0.5)));
        let none = stats(&[None, None]);
        assert_eq!((none.n, none.failed, none.mean), (0, 2, None));
        // a decision: the mean is the fraction of samples that gave 1
        assert_eq!(stats(&[Some(1.0), Some(0.0), Some(1.0), Some(1.0)]).mean, Some(0.75));
    }
}
