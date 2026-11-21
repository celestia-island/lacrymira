//! PostgreSQL dialect rendering: `Command` → SQL text + parameters.

use lacrymira_core::{Command, Condition, ConflictAction, Value};

/// A rendered SQL statement with positional parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    /// The SQL text with $1, $2, ... placeholders.
    pub sql: String,
    /// The bound parameter values.
    pub params: Vec<Value>,
}

/// Render a `Command` to PostgreSQL SQL.
pub fn render(cmd: &Command) -> Result<Rendered, lacrymira_core::Error> {
    let mut ctx = RenderCtx::new();
    match cmd {
        Command::Sql { text, params, .. } => Ok(Rendered {
            sql: text.clone(),
            params: params.clone(),
        }),
        Command::Select {
            table,
            filter,
            order_by,
            limit,
            offset,
            lock: lock_request,
        } => {
            let mut sql = format!("SELECT * FROM {}", quote_ident(table));
            if let Some(cond) = filter {
                let where_clause = ctx.render_condition(cond)?;
                sql.push_str(" WHERE ");
                sql.push_str(&where_clause);
            }
            if !order_by.is_empty() {
                sql.push_str(" ORDER BY ");
                let parts: Vec<String> = order_by
                    .iter()
                    .map(|(col, desc)| {
                        format!(
                            "{} {}",
                            quote_ident(col),
                            if *desc { "DESC" } else { "ASC" }
                        )
                    })
                    .collect();
                sql.push_str(&parts.join(", "));
            }
            if let Some(l) = limit {
                sql.push_str(&format!(" LIMIT {}", l));
            }
            if let Some(o) = offset {
                sql.push_str(&format!(" OFFSET {}", o));
            }
            // Pessimistic lock clause (FOR UPDATE family)
            if let Some(ref lock_req) = lock_request {
                sql.push(' ');
                sql.push_str(lock_req.sql());
            }
            Ok(Rendered {
                sql,
                params: ctx.params,
            })
        }
        Command::Insert {
            table,
            columns,
            rows,
            on_conflict,
        } => {
            let col_list = columns
                .iter()
                .map(|c| quote_ident(c))
                .collect::<Vec<_>>()
                .join(", ");
            let mut sql = format!("INSERT INTO {} ({})", quote_ident(table), col_list);

            // Value placeholders
            let placeholders: Vec<String> = rows
                .iter()
                .map(|row| {
                    let row_ph: Vec<String> = row
                        .iter()
                        .map(|v| {
                            ctx.bind(v.clone());
                            format!("${}", ctx.params.len())
                        })
                        .collect();
                    format!("({})", row_ph.join(", "))
                })
                .collect();
            sql.push_str(" VALUES ");
            sql.push_str(&placeholders.join(", "));

            // ON CONFLICT
            if let Some(oc) = on_conflict {
                let target = oc
                    .target
                    .iter()
                    .map(|c| quote_ident(c))
                    .collect::<Vec<_>>()
                    .join(", ");
                match &oc.action {
                    ConflictAction::DoNothing => {
                        sql.push_str(&format!(" ON CONFLICT ({}) DO NOTHING", target));
                    }
                    ConflictAction::DoUpdate { set } => {
                        sql.push_str(&format!(" ON CONFLICT ({}) DO UPDATE SET ", target));
                        let sets: Vec<String> = set
                            .iter()
                            .map(|(col, val)| {
                                // EXCLUDED.col references the incoming row
                                if let Value::Text(s) = val {
                                    if s.starts_with("EXCLUDED.") {
                                        return format!("{} = {}", quote_ident(col), s);
                                    }
                                }
                                ctx.bind(val.clone());
                                format!("{} = ${}", quote_ident(col), ctx.params.len())
                            })
                            .collect();
                        sql.push_str(&sets.join(", "));
                    }
                }
            }

            sql.push_str(" RETURNING *");
            Ok(Rendered {
                sql,
                params: ctx.params,
            })
        }
        Command::Update {
            table,
            sets,
            filter,
        } => {
            let mut sql = format!("UPDATE {} SET ", quote_ident(table));
            let set_parts: Vec<String> = sets
                .iter()
                .map(|(col, val)| {
                    ctx.bind(val.clone());
                    format!("{} = ${}", quote_ident(col), ctx.params.len())
                })
                .collect();
            sql.push_str(&set_parts.join(", "));
            if let Some(cond) = filter {
                let where_clause = ctx.render_condition(cond)?;
                sql.push_str(" WHERE ");
                sql.push_str(&where_clause);
            }
            sql.push_str(" RETURNING *");
            Ok(Rendered {
                sql,
                params: ctx.params,
            })
        }
        Command::Delete { table, filter } => {
            let mut sql = format!("DELETE FROM {}", quote_ident(table));
            if let Some(cond) = filter {
                let where_clause = ctx.render_condition(cond)?;
                sql.push_str(" WHERE ");
                sql.push_str(&where_clause);
            }
            Ok(Rendered {
                sql,
                params: ctx.params,
            })
        }
        Command::Kv { .. } => Err(lacrymira_core::Error::Config(
            "KV commands are not supported by the PostgreSQL driver".into(),
        )),
    }
}

/// Quote an identifier (double quotes, escape embedded quotes).
fn json_path_to_sql(path: &str) -> String {
    path.trim_start_matches('$')
        .trim_start_matches('.')
        .split('.')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("'->'")
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Rendering context: tracks the parameter counter.
struct RenderCtx {
    params: Vec<Value>,
}

impl RenderCtx {
    fn new() -> Self {
        RenderCtx { params: Vec::new() }
    }

    fn bind(&mut self, v: Value) {
        self.params.push(v);
    }

    fn render_condition(&mut self, cond: &Condition) -> Result<String, lacrymira_core::Error> {
        match cond {
            Condition::Eq(col, v) => Ok(self.render_cmp(col, "=", v)),
            Condition::Ne(col, v) => Ok(self.render_cmp(col, "!=", v)),
            Condition::Lt(col, v) => Ok(self.render_cmp(col, "<", v)),
            Condition::Lte(col, v) => Ok(self.render_cmp(col, "<=", v)),
            Condition::Gt(col, v) => Ok(self.render_cmp(col, ">", v)),
            Condition::Gte(col, v) => Ok(self.render_cmp(col, ">=", v)),
            Condition::Like(col, v) => Ok(self.render_cmp(col, "LIKE", v)),
            Condition::Ilike(col, v) => Ok(self.render_cmp(col, "ILIKE", v)),
            Condition::IsNull(col) => Ok(format!("{} IS NULL", quote_ident(&col.name))),
            Condition::IsNotNull(col) => Ok(format!("{} IS NOT NULL", quote_ident(&col.name))),
            Condition::In(col, values) => {
                // = ANY($n) — statement-cache friendly (constant arity)
                self.bind(Value::Array(values.clone()));
                Ok(format!(
                    "{} = ANY(${})",
                    quote_ident(&col.name),
                    self.params.len()
                ))
            }
            Condition::NotIn(col, values) => {
                self.bind(Value::Array(values.clone()));
                Ok(format!(
                    "{} != ANY(${})",
                    quote_ident(&col.name),
                    self.params.len()
                ))
            }
            Condition::JsonGetEq(col, path, v) => {
                self.bind(v.clone());
                let path_lit = json_path_to_sql(path);
                Ok(format!(
                    "{}->>'{}' = ${}",
                    quote_ident(&col.name),
                    path_lit,
                    self.params.len()
                ))
            }
            Condition::JsonExists(col, path) => {
                // $.a.b.c → col->'a'->'b' ? 'c'
                // $.a → col ? 'a'
                let segments: Vec<&str> = path
                    .trim_start_matches('$')
                    .trim_start_matches('.')
                    .split('.')
                    .filter(|s| !s.is_empty())
                    .collect();
                if segments.len() <= 1 {
                    Ok(format!("{} ?", quote_ident(&col.name)))
                } else {
                    let (parents, last) = segments.split_at(segments.len() - 1);
                    let chain = parents
                        .iter()
                        .map(|s| format!("->'{}'", s))
                        .collect::<String>();
                    Ok(format!(
                        "{}{} ? '{}'",
                        quote_ident(&col.name),
                        chain,
                        last[0]
                    ))
                }
            }
            Condition::JsonContains(col, v) => {
                self.bind(v.clone());
                Ok(format!(
                    "{} @> ${}::jsonb",
                    quote_ident(&col.name),
                    self.params.len()
                ))
            }
            Condition::JsonbPathMatch(col, path, vars) => {
                self.bind(Value::Text(path.clone()));
                self.bind(vars.clone());
                Ok(format!(
                    "jsonb_path_match({}, ${}, ${}::jsonb)",
                    quote_ident(&col.name),
                    self.params.len() - 1,
                    self.params.len()
                ))
            }
            Condition::Raw(sql, params) => {
                // Replace ${} placeholders with actual $n
                let mut result = sql.clone();
                for p in params {
                    self.bind(p.clone());
                    result = result.replacen("${}", &format!("${}", self.params.len()), 1);
                }
                Ok(result)
            }
            Condition::All(conds) => {
                let parts: Result<Vec<String>, _> =
                    conds.iter().map(|c| self.render_condition(c)).collect();
                Ok(format!("({})", parts?.join(" AND ")))
            }
            Condition::Any(conds) => {
                let parts: Result<Vec<String>, _> =
                    conds.iter().map(|c| self.render_condition(c)).collect();
                Ok(format!("({})", parts?.join(" OR ")))
            }
            Condition::Not(inner) => Ok(format!("NOT ({})", self.render_condition(inner)?)),
        }
    }

    fn render_cmp(&mut self, col: &lacrymira_core::ColumnRef, op: &str, v: &Value) -> String {
        match v {
            Value::Null => {
                // = NULL → IS NULL, != NULL → IS NOT NULL
                match op {
                    "=" => format!("{} IS NULL", quote_ident(&col.name)),
                    "!=" => format!("{} IS NOT NULL", quote_ident(&col.name)),
                    _ => format!("{} {} NULL", quote_ident(&col.name), op),
                }
            }
            _ => {
                self.bind(v.clone());
                format!("{} {} ${}", quote_ident(&col.name), op, self.params.len())
            }
        }
    }
}
