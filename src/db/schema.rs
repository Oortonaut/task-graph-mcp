//! Schema introspection queries for the task-graph database.

use super::Database;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Information about a table column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub primary_key: bool,
}

/// Information about an index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub table_name: String,
    pub unique: bool,
    pub columns: Vec<String>,
}

/// Information about a foreign key relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyInfo {
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    pub on_update: String,
    pub on_delete: String,
}

/// Information about a table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub name: String,
    pub table_type: String, // "table" or "view"
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<IndexInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub sql: Option<String>,
}

/// Complete database schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSchema {
    pub tables: Vec<TableInfo>,
    pub sqlite_version: String,
}

impl Database {
    /// Get complete schema information for the database.
    pub fn get_schema(&self, include_sql: bool) -> Result<DatabaseSchema> {
        self.with_conn(|conn| {
            // Get SQLite version
            let sqlite_version: String = conn.query_row(
                "SELECT sqlite_version()",
                [],
                |row| row.get(0),
            )?;

            // Get all tables and views (excluding internal sqlite_ tables and refinery schema)
            let mut stmt = conn.prepare(
                "SELECT name, type, sql FROM sqlite_master 
                 WHERE type IN ('table', 'view') 
                 AND name NOT LIKE 'sqlite_%'
                 AND name NOT LIKE 'refinery_%'
                 ORDER BY type DESC, name"
            )?;

            let table_names: Vec<(String, String, Option<String>)> = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut tables = Vec::new();

            for (table_name, table_type, sql) in table_names {
                // Get column info using PRAGMA table_info
                let columns = self.get_table_columns(conn, &table_name)?;

                // Get indexes for this table
                let indexes = self.get_table_indexes(conn, &table_name)?;

                // Get foreign keys for this table
                let foreign_keys = self.get_table_foreign_keys(conn, &table_name)?;

                tables.push(TableInfo {
                    name: table_name,
                    table_type,
                    columns,
                    indexes,
                    foreign_keys,
                    sql: if include_sql { sql } else { None },
                });
            }

            Ok(DatabaseSchema {
                tables,
                sqlite_version,
            })
        })
    }

    /// Get column information for a table.
    fn get_table_columns(&self, conn: &rusqlite::Connection, table_name: &str) -> Result<Vec<ColumnInfo>> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info('{}')", table_name))?;

        let columns: Vec<ColumnInfo> = stmt
            .query_map([], |row| {
                Ok(ColumnInfo {
                    name: row.get(1)?,
                    data_type: row.get::<_, String>(2)?.to_uppercase(),
                    nullable: row.get::<_, i32>(3)? == 0,
                    default_value: row.get(4)?,
                    primary_key: row.get::<_, i32>(5)? > 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(columns)
    }

    /// Get index information for a table.
    fn get_table_indexes(&self, conn: &rusqlite::Connection, table_name: &str) -> Result<Vec<IndexInfo>> {
        // Get list of indexes
        let mut stmt = conn.prepare(&format!("PRAGMA index_list('{}')", table_name))?;

        let index_list: Vec<(String, bool)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)? == 1, // unique
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut indexes = Vec::new();

        for (index_name, unique) in index_list {
            // Skip auto-generated indexes for primary keys
            if index_name.starts_with("sqlite_autoindex_") {
                continue;
            }

            // Get columns in this index
            let mut stmt = conn.prepare(&format!("PRAGMA index_info('{}')", index_name))?;

            let columns: Vec<String> = stmt
                .query_map([], |row| row.get(2))?
                .collect::<Result<Vec<_>, _>>()?;

            indexes.push(IndexInfo {
                name: index_name,
                table_name: table_name.to_string(),
                unique,
                columns,
            });
        }

        Ok(indexes)
    }

    /// Get foreign key information for a table.
    fn get_table_foreign_keys(&self, conn: &rusqlite::Connection, table_name: &str) -> Result<Vec<ForeignKeyInfo>> {
        let mut stmt = conn.prepare(&format!("PRAGMA foreign_key_list('{}')", table_name))?;

        let foreign_keys: Vec<ForeignKeyInfo> = stmt
            .query_map([], |row| {
                Ok(ForeignKeyInfo {
                    from_column: row.get(3)?,
                    to_table: row.get(2)?,
                    to_column: row.get(4)?,
                    on_update: row.get(5)?,
                    on_delete: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(foreign_keys)
    }

    /// Get a list of table names only (lightweight).
    pub fn get_table_names(&self) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master 
                 WHERE type = 'table' 
                 AND name NOT LIKE 'sqlite_%'
                 AND name NOT LIKE 'refinery_%'
                 ORDER BY name"
            )?;

            let names: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(names)
        })
    }
}
