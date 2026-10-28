//! Immutable document snapshots as Arrow IPC streams in sealed shared memory.

use std::sync::Arc;

use arrow_array::{
    ArrayRef, Int8Array, RecordBatch, StringArray, UInt8Array, UInt32Array, UInt64Array,
};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema};
use basecraft_docs::SequenceDoc;
use basecraft_platform::shm::{SharedBuffer, SharedWriter};
use basecraft_protocol::Strand;
use basecraft_protocol::plugin::SequenceSnapshot;

use crate::Result;

pub struct Snapshot {
    pub doc: String,
    pub name: String,
    pub version: u64,
    pub len: u64,
    pub circular: bool,
    pub residues: SharedBuffer,
    pub features: SharedBuffer,
}

fn write_stream(name: &str, batch: &RecordBatch) -> Result<SharedBuffer> {
    let mut w = SharedWriter::new(name)?;
    {
        let mut stream = StreamWriter::try_new(&mut w, &batch.schema())?;
        stream.write(batch)?;
        stream.finish()?;
    }
    Ok(w.seal()?)
}

impl Snapshot {
    /// Snapshot `doc` at its current version. The residues are copied once,
    /// from the piece table into shared memory; the plugin maps them without
    /// copying.
    pub fn of(key: &str, doc: &SequenceDoc) -> Result<Self> {
        let residues = UInt8Array::from(doc.read(0, doc.len()));
        let schema = Arc::new(Schema::new(vec![Field::new(
            "residues",
            DataType::UInt8,
            false,
        )]));
        let batch = RecordBatch::try_new(schema, vec![Arc::new(residues) as ArrayRef])?;
        let residues = write_stream("basecraft-residues", &batch)?;

        let (mut ids, mut kinds, mut starts, mut ends, mut strands, mut labels) = (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        for (id, f) in doc.features() {
            for s in &f.location.segments {
                ids.push(id.0);
                kinds.push(f.kind.clone());
                starts.push(s.start);
                ends.push(s.end);
                strands.push(match s.strand {
                    Strand::Forward => 1i8,
                    Strand::Reverse => -1,
                    Strand::Unstranded => 0,
                });
                labels.push(f.label().map(str::to_string));
            }
        }
        let schema = Arc::new(Schema::new(vec![
            Field::new("feature", DataType::UInt32, false),
            Field::new("kind", DataType::Utf8, false),
            Field::new("start", DataType::UInt64, false),
            Field::new("end", DataType::UInt64, false),
            Field::new("strand", DataType::Int8, false),
            Field::new("label", DataType::Utf8, true),
        ]));
        let columns: Vec<ArrayRef> = vec![
            Arc::new(UInt32Array::from(ids)),
            Arc::new(StringArray::from(kinds)),
            Arc::new(UInt64Array::from(starts)),
            Arc::new(UInt64Array::from(ends)),
            Arc::new(Int8Array::from(strands)),
            Arc::new(StringArray::from(labels)),
        ];
        let features = write_stream(
            "basecraft-features",
            &RecordBatch::try_new(schema, columns)?,
        )?;

        Ok(Self {
            doc: key.to_string(),
            name: doc.name.clone(),
            version: doc.version(),
            len: doc.len(),
            circular: doc.circular,
            residues,
            features,
        })
    }

    /// The exact submitted input as registered runs retain it
    /// (`sequence-arrow-buffers-v1`): the residue stream length, then the
    /// residue and feature streams.
    pub fn payload(&self) -> Result<Vec<u8>> {
        let residues = self.residues.map()?;
        let features = self.features.map()?;
        let mut payload = Vec::with_capacity(8 + residues.len() + features.len());
        payload.extend_from_slice(&(residues.len() as u64).to_le_bytes());
        payload.extend_from_slice(&residues);
        payload.extend_from_slice(&features);
        Ok(payload)
    }

    /// The protocol description, with buffer indices 0 and 1.
    pub fn describe(&self) -> SequenceSnapshot {
        SequenceSnapshot {
            doc: self.doc.clone(),
            name: self.name.clone(),
            version: self.version,
            len: self.len,
            circular: self.circular,
            residues: 0,
            features: 1,
        }
    }
}
