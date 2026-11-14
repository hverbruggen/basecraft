#[test]
fn reviewed_consensus_keeps_one_row_per_base() {
    let page = basecraft_viewmodel::accessible::review(b"AC", &[false, true], &[true, false], 0);
    assert_eq!(
        page.text,
        "Reviewed consensus. Columns are one-based.\nColumn\tBase\tMasked\tUnresolved\n1\tA\tno\tyes\n2\tC\tyes\tno\n"
    );
}
