use crate::value::Value;

#[derive(Debug, Clone)]
pub struct Row {
    pub values: Vec<Value>,
    pub schema: Vec<(String, catalog::DataType)>,
}

impl Row {
    pub fn new(values: Vec<Value>, schema: Vec<(String, catalog::DataType)>) -> Self {
        Self { values, schema }
    }

    pub fn get(&self, col_name: &str) -> Option<&Value> {
        // Exact match first
        if let Some(idx) = self.schema.iter().position(|(name, _)| name == col_name) {
            return self.values.get(idx);
        }
        // If unqualified name, try suffix match "alias.col"
        if !col_name.contains('.') {
            let suffix = format!(".{}", col_name);
            let matches: Vec<usize> = self
                .schema
                .iter()
                .enumerate()
                .filter(|(_, (name, _))| name.ends_with(&suffix))
                .map(|(i, _)| i)
                .collect();
            if matches.len() == 1 {
                return self.values.get(matches[0]);
            }
            // Ambiguous or not found — return None
        }
        None
    }

    pub fn get_by_idx(&self, idx: usize) -> Option<&Value> {
        self.values.get(idx)
    }
}
