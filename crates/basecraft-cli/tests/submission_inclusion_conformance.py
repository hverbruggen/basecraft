"""Focused CLI/Python/MCP inclusion parity against a built executable; no compilation.

PYTHONPATH=python/src python submission_inclusion_conformance.py --binary /abs/basecraft --root /external/new-directory
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

from basecraft.session import Session
from operation_conformance import Mcp


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ, BASECRAFT_PROFILE_DIR=str(root / 'profile'))
    os.environ['BASECRAFT_PROFILE_DIR'] = env['BASECRAFT_PROFILE_DIR']
    fasta = root / 'names.fas'
    fasta.write_text('>a_b_HV00145_XX description_is_not_identifier\nACGT\n>a_b_c_AM049955\nACGT\n>missing\nACGT\n>a_b_c_\nACGT\n')
    document = root / 'study.submission.json'

    def cli(*arguments):
        result = subprocess.run([str(binary), 'submission', *arguments, '--project', str(root)],
                                cwd=root, env=env, check=True, text=True, capture_output=True)
        return json.loads(result.stdout)

    cli('new', str(document), '--alignment', str(fasta))
    rule = dict(name='fourth field', input=dict(source='field', separator='_', index=4),
                mode='exact', pattern='XX', case_sensitive=True, trim=True,
                matching='include', nonmatching='exclude', unresolved='unchanged', override_manual=False)
    plan = cli('inclusion-preview', str(document), json.dumps(rule))
    assert (plan['matching'], plan['unresolved'], plan['included']) == (1, 2, 3), plan
    assert plan['rows'][0]['record'] == 'a_b_HV00145_XX'
    assert plan['rows'][2]['extracted'] is None
    assert plan['rows'][3]['extracted'] == ''
    with Session(root, command=str(binary), writes=True) as session:
        doc = session.open(document)['document']
        assert session.preview_submission_inclusion(doc, rule) == plan
        session.apply_submission_inclusion(doc, plan)
        applied = session.read(doc)
        assert len(applied['rows']) == 2
        assert len(applied['row_rules']) == 2
        session.undo(doc)
        assert session.read(doc)['rows'] == []
        session.redo(doc)
        assert session.read(doc) == applied
        stale = session.call('apply', {'document': doc, 'version': session.state(doc)['version'],
            'operation': {'op': 'submission.edit', 'params': {'edit': {'action': 'apply_inclusion', 'plan': plan}}}}, check=False)
        assert 'Stale' in str(stale), stale
        session.undo(doc)
        assert session.preview_submission_inclusion(doc, rule) == plan
    mcp = Mcp(binary, root, env, writable=False)
    try:
        doc = mcp.tool('open', {'path': str(document)})['structuredContent']['document']
        assert mcp.tool('submission_inclusion_preview', {'document': doc, 'rule': rule})['structuredContent'] == plan
        denied = mcp.tool('submission_inclusion_presets', {'save': rule})
        assert denied.get('isError') and 'permission_denied' in str(denied), denied
        assert mcp.tool('submission_inclusion_presets', {})['structuredContent'] == []
    finally:
        mcp.close()
    cli('inclusion-preset', json.dumps(rule))
    with Session(root, command=str(binary)) as session:
        assert session.submission_inclusion_presets() == [rule]
    # Saving presets changes metadata evidence, so the original plan must now be stale.
    refused = subprocess.run([str(binary), 'submission', 'edit', str(document),
        json.dumps(dict(action='apply_inclusion', plan=plan)), '--project', str(root)], env=env, capture_output=True, text=True)
    assert refused.returncode != 0 and 'Stale' in refused.stderr, refused.stderr
    fresh = cli('inclusion-preview', str(document), json.dumps(rule))
    plan_file = root / 'apply.json'
    plan_file.write_text(json.dumps(dict(action='apply_inclusion', plan=fresh)))
    cli('edit', str(document), '@' + str(plan_file))
    assert len(json.loads(document.read_text())['rows']) == 2
    print(json.dumps(dict(passed=True, interfaces=['CLI', 'Python', 'MCP'], rows=4)))


if __name__ == '__main__':
    main()
