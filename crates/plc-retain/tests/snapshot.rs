//! Snapshot buffer publish / take coalescing.

use plc_retain::RetainSnapshotBuffer;

#[test]
fn snapshot_publish_read() {
    let buf = RetainSnapshotBuffer::new(4);
    let mut dst = [0u8; 4];
    assert!(buf.read(&mut dst).is_none());

    buf.publish(&[1, 2, 3, 4]);
    let first = buf.read(&mut dst).expect("first publish");
    assert!(first >= 1);
    assert_eq!(dst, [1, 2, 3, 4]);
    assert!(buf.read(&mut dst).is_none());

    buf.publish(&[9, 9, 9, 9]);
    buf.publish(&[5, 6, 7, 8]);
    let second = buf.read(&mut dst).expect("coalesced");
    assert!(second > first);
    assert_eq!(dst, [5, 6, 7, 8]);
    assert_eq!(buf.seq(), second);
    assert!(buf.read(&mut dst).is_none());
}
