//! Small, deterministic arithmetic expressions. No scripting or host-code execution.
use std::collections::BTreeMap;

/// Evaluate numbers, variables, + - * / %, parentheses and the whitelisted
/// functions min/max/abs/sqrt/pow/floor/ceil/round (optionally prefixed `Math.`).
/// Both comma and semicolon argument separators and an optional leading `=`
/// are accepted.
pub fn evaluate_expression(
    source: &str,
    mut variable: impl FnMut(&str) -> Result<f64, String>,
) -> Result<f64, String> {
    let source = source.trim().strip_prefix('=').unwrap_or(source.trim());
    if source.len() > 4096 {
        return Err("Expression exceeds 4096 characters".into());
    }
    let mut parser = Parser {
        source: source.as_bytes(),
        pos: 0,
        depth: 0,
        variable: &mut variable,
    };
    let value = parser.sum()?;
    parser.space();
    if parser.pos != parser.source.len() {
        return Err(format!(
            "Unexpected character at position {}",
            parser.pos + 1
        ));
    }
    finite(value)
}

/// Resolve a set of named expressions, including references to other parameters.
/// A cycle is an error, even if cached numeric values happen to be available.
pub fn evaluate_parameters(
    sources: &BTreeMap<String, String>,
    variables: &BTreeMap<String, f64>,
) -> Result<BTreeMap<String, f64>, String> {
    evaluate_parameter_results(sources, variables)
        .into_iter()
        .map(|(name, value)| value.map(|value| (name, value)))
        .collect()
}

/// Resolve each parameter and its dependencies independently. An error in one
/// expression affects that expression and its actual dependents, never its peers.
pub fn evaluate_parameter_results(
    sources: &BTreeMap<String, String>,
    variables: &BTreeMap<String, f64>,
) -> BTreeMap<String, Result<f64, String>> {
    evaluate_parameter_results_with(sources, variables, &BTreeMap::new(), |_, _| Ok(()))
}

pub(crate) fn evaluate_parameter_results_with(
    sources: &BTreeMap<String, String>,
    variables: &BTreeMap<String, f64>,
    source_errors: &BTreeMap<String, String>,
    validate: impl Fn(&str, f64) -> Result<(), String>,
) -> BTreeMap<String, Result<f64, String>> {
    let mut values = BTreeMap::new();
    sources
        .keys()
        .map(|name| {
            let value = resolve(
                name,
                sources,
                variables,
                &mut values,
                &mut Vec::new(),
                source_errors,
                &validate,
            );
            (name.clone(), value)
        })
        .collect()
}

fn resolve(
    name: &str,
    sources: &BTreeMap<String, String>,
    variables: &BTreeMap<String, f64>,
    values: &mut BTreeMap<String, f64>,
    stack: &mut Vec<String>,
    source_errors: &BTreeMap<String, String>,
    validate: &impl Fn(&str, f64) -> Result<(), String>,
) -> Result<f64, String> {
    if let Some(value) = values.get(name).or_else(|| variables.get(name)) {
        return finite(*value);
    }
    if stack.iter().any(|entry| entry == name) {
        return Err(format!(
            "Circular parameter reference: {} → {name}",
            stack.join(" → ")
        ));
    }
    if stack.len() >= 64 {
        return Err("Parameter dependency limit exceeded".into());
    }
    if let Some(error) = source_errors.get(name) {
        return Err(format!("{name}: {error}"));
    }
    let source = sources
        .get(name)
        .ok_or_else(|| format!("Unknown parameter: {name}"))?;
    stack.push(name.into());
    let result = evaluate_expression(source, |dependency| {
        resolve(
            dependency,
            sources,
            variables,
            values,
            stack,
            source_errors,
            validate,
        )
    });
    stack.pop();
    let value = result.map_err(|error| format!("{name}: {error}"))?;
    validate(name, value).map_err(|error| format!("{name}: {error}"))?;
    values.insert(name.into(), value);
    Ok(value)
}

fn finite(value: f64) -> Result<f64, String> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err("Expression must produce a finite number".into())
    }
}

struct Parser<'a, F> {
    source: &'a [u8],
    pos: usize,
    depth: usize,
    variable: &'a mut F,
}
impl<F: FnMut(&str) -> Result<f64, String>> Parser<'_, F> {
    fn space(&mut self) {
        while self
            .source
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn take(&mut self, byte: u8) -> bool {
        self.space();
        if self.source.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn sum(&mut self) -> Result<f64, String> {
        let mut value = self.product()?;
        loop {
            if self.take(b'+') {
                value = finite(value + self.product()?)?;
            } else if self.take(b'-') {
                value = finite(value - self.product()?)?;
            } else {
                return Ok(value);
            }
        }
    }
    fn product(&mut self) -> Result<f64, String> {
        let mut value = self.atom()?;
        loop {
            if self.take(b'*') {
                value = finite(value * self.atom()?)?;
            } else if self.take(b'/') {
                let divisor = self.atom()?;
                if divisor == 0. {
                    return Err("Division by zero".into());
                }
                value = finite(value / divisor)?;
            } else if self.take(b'%') {
                let divisor = self.atom()?;
                if divisor == 0. {
                    return Err("Remainder by zero".into());
                }
                value = finite(value % divisor)?;
            } else {
                return Ok(value);
            }
        }
    }
    fn atom(&mut self) -> Result<f64, String> {
        if self.depth >= 64 {
            return Err("Expression nesting limit exceeded".into());
        }
        self.depth += 1;
        let result = self.atom_inner();
        self.depth -= 1;
        result
    }
    fn atom_inner(&mut self) -> Result<f64, String> {
        if self.take(b'+') {
            return self.atom();
        }
        if self.take(b'-') {
            return finite(-self.atom()?);
        }
        if self.take(b'(') {
            let value = self.sum()?;
            if !self.take(b')') {
                return Err("Expected ')'".into());
            }
            return Ok(value);
        }
        self.space();
        let start = self.pos;
        if self
            .source
            .get(start)
            .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
        {
            while self
                .source
                .get(self.pos)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
            {
                self.pos += 1;
            }
            if self
                .source
                .get(self.pos)
                .is_some_and(|b| *b == b'e' || *b == b'E')
            {
                self.pos += 1;
                if self
                    .source
                    .get(self.pos)
                    .is_some_and(|b| *b == b'+' || *b == b'-')
                {
                    self.pos += 1;
                }
                while self.source.get(self.pos).is_some_and(u8::is_ascii_digit) {
                    self.pos += 1;
                }
            }
            return std::str::from_utf8(&self.source[start..self.pos])
                .unwrap()
                .parse::<f64>()
                .map_err(|_| "Invalid number".into())
                .and_then(finite);
        }
        while self
            .source
            .get(self.pos)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.')
        {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(format!(
                "Expected a number or parameter at position {}",
                start + 1
            ));
        }
        let name = std::str::from_utf8(&self.source[start..self.pos]).unwrap();
        if !self.take(b'(') {
            return (self.variable)(name).and_then(finite);
        }
        let mut args = vec![self.sum()?];
        while self.take(b',') || self.take(b';') {
            args.push(self.sum()?);
        }
        if !self.take(b')') {
            return Err("Expected ')' after function arguments".into());
        }
        let value = match (name.strip_prefix("Math.").unwrap_or(name), args.as_slice()) {
            ("min", [first, rest @ ..]) => rest.iter().fold(*first, |a, b| a.min(*b)),
            ("max", [first, rest @ ..]) => rest.iter().fold(*first, |a, b| a.max(*b)),
            ("abs", [a]) => a.abs(),
            ("sqrt", [a]) => a.sqrt(),
            ("floor", [a]) => a.floor(),
            ("ceil", [a]) => a.ceil(),
            ("round", [a]) => a.round(),
            ("pow", [a, b]) => a.powf(*b),
            _ => return Err(format!("Unknown function or wrong argument count: {name}")),
        };
        finite(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn eval(s: &str) -> Result<f64, String> {
        evaluate_expression(s, |name| Err(format!("Unknown parameter: {name}")))
    }
    #[test]
    fn arithmetic_and_functions() {
        assert_eq!(eval("-2 + 3 * (4 - -1) / 2").unwrap(), 5.5);
        assert_eq!(eval(" = 6 * 0.1").unwrap(), 6. * 0.1);
        assert_eq!(eval("Math.max(2; min(3, 4)) + 1e-2").unwrap(), 3.01);
        assert_eq!(eval("pow(2,3) % 3 + sqrt(4)").unwrap(), 4.);
    }
    #[test]
    fn invalid_and_unsafe_expressions_fail() {
        for s in [
            "",
            "1/0",
            "1%0",
            "sqrt(-1)",
            "1e999",
            "2 +",
            "min()",
            "pow(2)",
            "1 2",
            "Math.random()",
            "process.exit()",
            "x = 2",
            "1;2",
        ] {
            assert!(eval(s).is_err(), "{s}");
        }
        assert!(eval(&"(".repeat(1000)).is_err());
        assert!(eval(&"1".repeat(4097)).is_err());
    }
    #[test]
    fn unrelated_errors_do_not_poison_independent_roots() {
        let sources = BTreeMap::from([
            ("ramp".into(), "=".into()),
            ("lead".into(), "ramp * 2".into()),
            ("feed".into(), "300".into()),
            ("step".into(), "diameter / 7".into()),
        ]);
        let variables = BTreeMap::from([("diameter".into(), 6.)]);
        let values = evaluate_parameter_results(&sources, &variables);
        assert!(values["ramp"].is_err());
        assert!(values["lead"].as_ref().unwrap_err().contains("ramp"));
        assert_eq!(values["feed"], Ok(300.));
        assert_eq!(values["step"], Ok(6. / 7.));
        assert!(evaluate_parameters(&sources, &variables).is_err());
    }
    #[test]
    fn dependencies_and_cycles() {
        let mut sources = BTreeMap::from([
            ("a".into(), "b * 2".into()),
            ("b".into(), "diameter * 0.1".into()),
        ]);
        let vars = BTreeMap::from([("diameter".into(), 6.)]);
        assert!((evaluate_parameters(&sources, &vars).unwrap()["a"] - 1.2).abs() < 1e-12);
        sources.insert("b".into(), "a".into());
        assert!(evaluate_parameters(&sources, &vars)
            .unwrap_err()
            .contains("Circular"));
    }
}
