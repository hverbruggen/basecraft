"""Check all public entry routes against a built CLI and a transport-only mock.

Run explicitly with --binary and a new disposable --root. No service contact.
The unmodified query preparation, adapter, job lifecycle and artifact store run.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

MOCK = '''
import os
from pathlib import Path
import blast_plugin as adapter
def transport(fields):
    assert fields['CMD']=='Put'
    assert fields['DATABASE']=='nt'
    assert fields['NO_DATABASE_OVERRIDE']=='true'
    assert fields['QUERY'].count('>')==2
    with Path(os.environ['BLAST_MOCK_CONTACTS']).open('a') as stream: stream.write('POST\\n')
    return 'QBlastInfoBegin\\nRID = ABCD1234567\\nQBlastInfoEnd'
adapter.submit_prepared.__defaults__=(transport,)
adapter.plugin.run()
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = args.root.absolute()
    root.mkdir()
    repository = Path(__file__).resolve().parents[2]
    package = root / 'bundle' / 'blast'
    shutil.copytree(Path(__file__).parent, package)
    manifest = package / 'plugin.toml'
    manifest.write_text(manifest.read_text().replace('python:blast_plugin.py', 'python:mock.py'))
    (package / 'mock.py').write_text(MOCK)
    contacts = root / 'contacts'
    env = dict(os.environ, BASECRAFT=str(binary), BASECRAFT_PROFILE_DIR=str(root / 'profile'),
               BASECRAFT_PLUGINS=str(package.parent), BASECRAFT_PYTHON=sys.executable,
               BASECRAFT_PYTHONPATH=str(repository / 'python/src'),
               BASECRAFT_CONTACT_EMAIL='test@example.org', BLAST_MOCK_CONTACTS=str(contacts))
    os.environ.update(env)
    sys.path.insert(0, str(repository / 'python/src'))
    sys.path.insert(0, str(repository / 'crates/basecraft-cli/tests'))
    from basecraft import operations
    from operation_conformance import Mcp

    project = root / 'project'
    project.mkdir()
    (project / 'queries.fa').write_text('>same\nACGT\n>same\nACGU\n')
    inputs = {'queries': [{'kind': 'file', 'path': 'queries.fa'}]}
    parameters = {'alphabet': 'nucleotide', 'database': 'nt'}
    operation = 'basecraft.blast/submit@1'
    request = {'operation': operation, 'inputs': inputs, 'parameters': parameters}
    request_file = project / 'request.json'
    request_file.write_text(json.dumps(request))
    completed = subprocess.run([str(binary), 'operations', 'run', operation, '--project', str(project),
                                '--request', str(request_file), '--json'], env=env, capture_output=True,
                               text=True, check=True, timeout=40)
    cli = json.loads(completed.stdout)
    python = operations.run(operation, project=str(project), inputs=inputs, parameters=parameters)
    mcp = Mcp(binary, project, env)
    try:
        tools = {tool['name'] for tool in mcp.tools()}
        assert 'basecraft.blast.submit.v1' in tools and 'basecraft.blast.receipt.v1' in tools
        accepted = mcp.tool('basecraft.blast.submit.v1', request)['structuredContent']
        remote = mcp.wait(accepted['run_id'])
    finally:
        mcp.close()
    for outcome in (cli, python, remote):
        assert outcome['state'] == 'completed', outcome
        data = outcome['data']
        assert data['status'] == 'accepted' and data['submitted_residues'] == 8, data
        assert [record['id'] for record in data['plan']['records']] == ['query_1', 'query_2']
        assert outcome['effective_parameters']['contact_email'] == 'test@example.org'
    assert contacts.read_text().splitlines() == ['POST'] * 3
    # New local read bookkeeping, with shared retained-input/artifact verification;
    # neither a new remote submission nor recreation of the original owned job.
    evidence = Path(cli['evidence'])
    recovered = operations.run('basecraft.blast/receipt@1', project=str(project), inputs={
        'receipt': [{'kind': 'file', 'path': cli['artifacts'][0]['path']}],
        'query': [{'kind': 'file', 'path': str(evidence / 'query-preparation.json')}],
    }, parameters={})
    assert recovered['data'] == cli['data']
    assert recovered['run_id'] != cli['run_id']
    assert contacts.read_text().splitlines() == ['POST'] * 3
    (root / 'receipt.json').write_text(json.dumps({'binary': str(binary), 'checks': [
        'CLI/Python/MCP same frozen preparation and receipt', 'unique IDs/all records',
        'configured contact', 'retained read uses shared verification without POST'], 'status': 'passed'}, indent=2))
    print('BLAST public-interface mock checks passed; no NCBI contact.')


if __name__ == '__main__':
    main()
