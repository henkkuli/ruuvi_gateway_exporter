use std::fmt;

#[derive(Clone)]
pub struct LabelSet<'a> {
    labels: Vec<(&'a str, &'a str)>,
}

pub struct MetricBuilder<'a> {
    name: &'a str,
    labels: Vec<(&'a str, &'a str)>,
}

pub struct Metric<'a, V> {
    name: &'a str,
    labels: Vec<(&'a str, &'a str)>,
    value: V,
}

pub fn metric(name: &str) -> MetricBuilder<'_> {
    MetricBuilder {
        name,
        labels: Vec::new(),
    }
}

pub fn labelset() -> LabelSet<'static> {
    LabelSet { labels: Vec::new() }
}

impl<'a> LabelSet<'a> {
    pub fn label(mut self, key: &'a str, value: &'a str) -> Self {
        self.labels.push((key, value));
        self
    }
}

impl<'a> MetricBuilder<'a> {
    pub fn labels(mut self, labelset: &LabelSet<'a>) -> Self {
        self.labels.extend(&labelset.labels);
        self
    }

    #[allow(dead_code)]
    pub fn label(mut self, key: &'a str, value: &'a str) -> Self {
        self.labels.push((key, value));
        self
    }

    pub fn value<V>(self, value: V) -> Metric<'a, V> {
        Metric {
            name: self.name,
            labels: self.labels,
            value,
        }
    }
}

impl<V: fmt::Display> fmt::Display for Metric<'_, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if !self.labels.is_empty() {
            write!(f, "{{")?;
            for (i, (key, value)) in self.labels.iter().enumerate() {
                if i > 0 {
                    write!(f, ",")?;
                }
                write!(f, "{key}=\"{value}\"")?;
            }
            write!(f, "}}")?;
        }
        write!(f, " {}", self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_formatting() {
        let m = metric("temperature")
            .label("sensor", "abc")
            .label("room", "kitchen")
            .value(23.5);
        assert_eq!(
            m.to_string(),
            "temperature{sensor=\"abc\",room=\"kitchen\"} 23.5"
        );

        // A metric without labels
        let m = metric("counter").value(42);
        assert_eq!(m.to_string(), "counter 42");

        // Test label sets
        let common_labels = labelset().label("datacenter", "eu-1").label("rack", "r42");

        let m = metric("temperature")
            .labels(&common_labels)
            .label("sensor", "abc")
            .value(23.5);

        assert_eq!(
            m.to_string(),
            "temperature{datacenter=\"eu-1\",rack=\"r42\",sensor=\"abc\"} 23.5"
        );

        let m = metric("humidity").labels(&common_labels).value(45);

        assert_eq!(
            m.to_string(),
            "humidity{datacenter=\"eu-1\",rack=\"r42\"} 45"
        );
    }
}
