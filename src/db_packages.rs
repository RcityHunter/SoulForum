#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnDef {
    pub name: &'static str,
    pub column_type: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableDef {
    pub name: &'static str,
    pub columns: &'static [ColumnDef],
}

pub fn pm_tables() -> Vec<TableDef> {
    vec![
        TableDef {
            name: "personal_messages",
            columns: &[
                ColumnDef {
                    name: "id_pm",
                    column_type: "int unsigned",
                },
                ColumnDef {
                    name: "id_pm_head",
                    column_type: "int unsigned",
                },
                ColumnDef {
                    name: "id_member_from",
                    column_type: "int unsigned",
                },
                ColumnDef {
                    name: "subject",
                    column_type: "varchar",
                },
                ColumnDef {
                    name: "body",
                    column_type: "text",
                },
            ],
        },
        TableDef {
            name: "pm_recipients",
            columns: &[
                ColumnDef {
                    name: "id_pm",
                    column_type: "int unsigned",
                },
                ColumnDef {
                    name: "id_member",
                    column_type: "int unsigned",
                },
                ColumnDef {
                    name: "is_read",
                    column_type: "tinyint",
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pm_table_definitions_exist() {
        let tables = pm_tables();
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].name, "personal_messages");
    }
}
