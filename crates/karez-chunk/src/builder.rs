use crate::column::{Column, FromColumn};
use crate::error::ChunkError;
use crate::types::Field;
use crate::{CHUNK_MAX_BYTES, CHUNK_MAX_ROWS};

type Result<T> = std::result::Result<T, ChunkError>;

/// A schema plus columns, all of the same length.
#[derive(Debug, Clone)]
pub struct Chunk {
    fields: Vec<Field>,
    columns: Vec<Column>,
    rows: usize,
}

impl Chunk {
    /// An empty chunk with the given schema.
    pub fn new(fields: Vec<Field>) -> Self {
        let columns = fields.iter().map(|f| Column::new(f.ty)).collect();
        Chunk {
            fields,
            columns,
            rows: 0,
        }
    }

    /// The schema.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// A column by index.
    pub fn column(&self, index: usize) -> Result<&Column> {
        self.columns.get(index).ok_or(ChunkError::OutOfBounds {
            index,
            len: self.columns.len(),
        })
    }

    /// Index of the first column with the given name.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    /// A borrowed view of one row.
    pub fn row(&self, index: usize) -> Result<RowRef<'_>> {
        if index >= self.rows {
            return Err(ChunkError::OutOfBounds {
                index,
                len: self.rows,
            });
        }
        Ok(RowRef {
            chunk: self,
            row: index,
        })
    }

    /// Approximate bytes held by all buffers.
    pub fn byte_size(&self) -> usize {
        self.columns.iter().map(Column::byte_size).sum()
    }

    /// Check that every column is consistent and has `rows` entries.
    pub fn validate(&self) -> Result<()> {
        if self.fields.len() != self.columns.len() {
            return Err(ChunkError::Corrupt("field and column counts differ"));
        }
        for (f, c) in self.fields.iter().zip(&self.columns) {
            if f.ty != c.ty() {
                return Err(ChunkError::Corrupt("column type differs from field"));
            }
            if c.len() != self.rows {
                return Err(ChunkError::Corrupt("column length differs from row count"));
            }
            c.validate()?;
        }
        Ok(())
    }
}

/// A borrowed view of one row of a [`Chunk`]. Reading allocates nothing: strings and byte
/// slices borrow from the chunk.
#[derive(Debug, Clone, Copy)]
pub struct RowRef<'a> {
    chunk: &'a Chunk,
    row: usize,
}

impl<'a> RowRef<'a> {
    /// Row index within the chunk.
    pub fn index(&self) -> usize {
        self.row
    }

    /// Read column `col` as `T`; use `Option<T>` to accept NULL.
    pub fn get<T: FromColumn<'a>>(&self, col: usize) -> Result<T> {
        self.chunk.column(col)?.get::<T>(self.row)
    }

    /// Read a column by name.
    pub fn get_by_name<T: FromColumn<'a>>(&self, name: &str) -> Result<T> {
        let idx = self
            .chunk
            .column_index(name)
            .ok_or(ChunkError::UnknownColumn)?;
        self.get(idx)
    }

    /// True when column `col` is NULL in this row.
    pub fn is_null(&self, col: usize) -> Result<bool> {
        self.chunk.column(col)?.is_null(self.row)
    }
}

/// Builds a [`Chunk`] row by row. Reuse it with [`ChunkBuilder::reset`]: buffers keep their
/// capacity, so a one-row result costs a reset, not an allocation.
///
/// Usage: for each row, push one value into every column via [`ChunkBuilder::column_mut`],
/// then call [`ChunkBuilder::finish_row`]. If an error interrupts a row, call
/// [`ChunkBuilder::abort_row`].
#[derive(Debug, Clone)]
pub struct ChunkBuilder {
    chunk: Chunk,
}

impl ChunkBuilder {
    /// A builder for the given schema.
    pub fn new(fields: Vec<Field>) -> Self {
        ChunkBuilder {
            chunk: Chunk::new(fields),
        }
    }

    /// Replace the schema (discarding rows), reusing column allocations where the types match.
    pub fn set_schema(&mut self, fields: Vec<Field>) {
        let mut old: Vec<Column> = std::mem::take(&mut self.chunk.columns);
        let mut columns = Vec::with_capacity(fields.len());
        for (i, f) in fields.iter().enumerate() {
            if let Some(c) = old.get_mut(i).filter(|c| c.ty() == f.ty) {
                let mut c = std::mem::replace(c, Column::new(f.ty));
                c.clear();
                columns.push(c);
            } else {
                columns.push(Column::new(f.ty));
            }
        }
        self.chunk = Chunk {
            fields,
            columns,
            rows: 0,
        };
    }

    /// Remove all rows, keeping the schema and the buffers' capacity.
    pub fn reset(&mut self) {
        for c in &mut self.chunk.columns {
            c.clear();
        }
        self.chunk.rows = 0;
    }

    /// The chunk built so far (only complete rows are counted).
    pub fn chunk(&self) -> &Chunk {
        &self.chunk
    }

    /// Give up the builder and return the chunk.
    pub fn into_chunk(self) -> Chunk {
        self.chunk
    }

    /// Number of finished rows.
    pub fn rows(&self) -> usize {
        self.chunk.rows
    }

    /// Mutable access to a column to push this row's value.
    pub fn column_mut(&mut self, index: usize) -> Result<&mut Column> {
        let len = self.chunk.columns.len();
        self.chunk
            .columns
            .get_mut(index)
            .ok_or(ChunkError::OutOfBounds { index, len })
    }

    /// Finish the current row. Every column must have exactly one more value than before.
    pub fn finish_row(&mut self) -> Result<()> {
        let want = self.chunk.rows.checked_add(1).ok_or(ChunkError::Overflow)?;
        if self.chunk.columns.iter().any(|c| c.len() != want) {
            return Err(ChunkError::RowIncomplete);
        }
        self.chunk.rows = want;
        Ok(())
    }

    /// Discard any partial row: every column is cut back to the finished row count.
    pub fn abort_row(&mut self) {
        let rows = self.chunk.rows;
        for c in &mut self.chunk.columns {
            c.truncate(rows);
        }
    }

    /// True once the chunk holds [`CHUNK_MAX_ROWS`] rows or [`CHUNK_MAX_BYTES`] bytes.
    pub fn is_full(&self) -> bool {
        self.chunk.rows >= CHUNK_MAX_ROWS || self.chunk.byte_size() >= CHUNK_MAX_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SplitMix64;
    use crate::types::ColumnType;

    fn schema() -> Vec<Field> {
        vec![
            Field::new("id", ColumnType::Int64),
            Field::new("name", ColumnType::Utf8),
            Field::new("ok", ColumnType::Bool),
        ]
    }

    #[test]
    fn builds_and_reads_rows() {
        let mut b = ChunkBuilder::new(schema());
        b.column_mut(0).unwrap().push_i64(7).unwrap();
        b.column_mut(1).unwrap().push_str("seven").unwrap();
        b.column_mut(2).unwrap().push_bool(true).unwrap();
        b.finish_row().unwrap();
        b.column_mut(0).unwrap().push_null().unwrap();
        b.column_mut(1).unwrap().push_null().unwrap();
        b.column_mut(2).unwrap().push_bool(false).unwrap();
        b.finish_row().unwrap();
        let c = b.chunk();
        c.validate().unwrap();
        assert_eq!(c.rows(), 2);
        let r0 = c.row(0).unwrap();
        assert_eq!(r0.get::<i64>(0).unwrap(), 7);
        assert_eq!(r0.get_by_name::<&str>("name").unwrap(), "seven");
        let r1 = c.row(1).unwrap();
        assert_eq!(r1.get::<Option<i64>>(0).unwrap(), None);
        assert!(r1.is_null(1).unwrap());
        assert!(!r1.get::<bool>(2).unwrap());
        assert!(c.row(2).is_err());
        assert_eq!(
            r0.get_by_name::<i64>("nope"),
            Err(ChunkError::UnknownColumn)
        );
    }

    #[test]
    fn incomplete_row_is_rejected_and_abort_rolls_back() {
        let mut b = ChunkBuilder::new(schema());
        b.column_mut(0).unwrap().push_i64(1).unwrap();
        assert_eq!(b.finish_row(), Err(ChunkError::RowIncomplete));
        b.abort_row();
        b.chunk().validate().unwrap();
        assert_eq!(b.rows(), 0);
        // Good row after the abort.
        b.column_mut(0).unwrap().push_i64(2).unwrap();
        b.column_mut(1).unwrap().push_str("x").unwrap();
        b.column_mut(2).unwrap().push_bool(true).unwrap();
        b.finish_row().unwrap();
        b.chunk().validate().unwrap();
    }

    #[test]
    fn reset_keeps_capacity() {
        let mut b = ChunkBuilder::new(schema());
        for i in 0..500 {
            b.column_mut(0).unwrap().push_i64(i).unwrap();
            b.column_mut(1).unwrap().push_str("some text here").unwrap();
            b.column_mut(2).unwrap().push_bool(i % 2 == 0).unwrap();
            b.finish_row().unwrap();
        }
        let cap = b.chunk().column(1).unwrap().data().len();
        assert!(cap > 0);
        b.reset();
        assert_eq!(b.rows(), 0);
        b.chunk().validate().unwrap();
        assert_eq!(b.chunk().column(1).unwrap().data().len(), 0);
        // Reusable after reset.
        b.column_mut(0).unwrap().push_i64(1).unwrap();
        b.column_mut(1).unwrap().push_str("a").unwrap();
        b.column_mut(2).unwrap().push_bool(true).unwrap();
        b.finish_row().unwrap();
        b.chunk().validate().unwrap();
    }

    #[test]
    fn full_after_max_rows() {
        let mut b = ChunkBuilder::new(vec![Field::new("a", ColumnType::Int16)]);
        for i in 0..CHUNK_MAX_ROWS {
            assert!(!b.is_full());
            b.column_mut(0).unwrap().push_i16(i as i16).unwrap();
            b.finish_row().unwrap();
        }
        assert!(b.is_full());
    }

    #[test]
    fn set_schema_changes_shape() {
        let mut b = ChunkBuilder::new(schema());
        b.column_mut(0).unwrap().push_i64(1).unwrap();
        b.set_schema(vec![Field::new("x", ColumnType::Float64)]);
        assert_eq!(b.rows(), 0);
        assert_eq!(b.chunk().column_count(), 1);
        b.column_mut(0).unwrap().push_f64(1.5).unwrap();
        b.finish_row().unwrap();
        b.chunk().validate().unwrap();
    }

    #[test]
    fn randomised_aborts_never_break_invariants() {
        let mut rng = SplitMix64::new(99);
        let mut b = ChunkBuilder::new(schema());
        for _ in 0..2000 {
            let upto = rng.range(0, 3);
            if upto > 0 {
                if rng.chance(1, 3) {
                    b.column_mut(0).unwrap().push_null().unwrap();
                } else {
                    b.column_mut(0)
                        .unwrap()
                        .push_i64(rng.next_u64() as i64)
                        .unwrap();
                }
            }
            if upto > 1 {
                b.column_mut(1).unwrap().push_str(&rng.string(8)).unwrap();
            }
            if upto > 2 {
                b.column_mut(2)
                    .unwrap()
                    .push_bool(rng.chance(1, 2))
                    .unwrap();
            }
            if b.finish_row().is_err() {
                b.abort_row();
            }
            if rng.chance(1, 200) {
                b.reset();
            }
            b.chunk().validate().unwrap();
        }
    }
}
