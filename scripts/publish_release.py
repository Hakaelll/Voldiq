"""Publish the verified installer using the configured Git credential helper."""

import argparse
import hashlib
import http.client
import json
import pathlib
import subprocess
import urllib.error
import urllib.parse
import urllib.request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--repo', required=True)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--asset', type=pathlib.Path, required=True)
    parser.add_argument('--notes', type=pathlib.Path, required=True)
    parser.add_argument('--receipt', type=pathlib.Path, required=True)
    args = parser.parse_args()
    subprocess.run(['git', 'diff', '--quiet', 'HEAD'], check=True)
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    tagged = subprocess.check_output(['git', 'rev-list', '-n', '1', args.tag], text=True).strip()
    if tagged != commit:
        raise RuntimeError('Release tag must identify the verified checkout.')
    credential = subprocess.run(
        ['git', 'credential', 'fill'], input='protocol=https\nhost=github.com\n\n',
        capture_output=True, text=True, check=True,
    )
    fields = dict(line.split('=', 1) for line in credential.stdout.splitlines() if '=' in line)
    token = fields.get('password')
    if not token:
        raise RuntimeError('The Git credential helper returned no GitHub credential.')
    headers = {
        'Authorization': f'Bearer {token}', 'Accept': 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'Voldiq-release',
    }

    def api(path, body=None, method=None):
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(
            f'https://api.github.com/repos/{args.repo}{path}', data=data,
            headers={**headers, 'Content-Type': 'application/json'}, method=method,
        )
        with urllib.request.urlopen(request, timeout=90) as response:
            return json.load(response)

    digest = hashlib.file_digest(args.asset.open('rb'), 'sha256').hexdigest()
    size = args.asset.stat().st_size
    notes = args.notes.read_text(encoding='utf-8')
    try:
        release = api(f'/releases/tags/{args.tag}')
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        release = api('/releases', {
            'tag_name': args.tag, 'target_commitish': commit,
            'name': f'Voldiq {args.tag.removeprefix("v")} · Skyline',
            'body': notes, 'draft': True, 'prerelease': False,
        })
    matches = [item for item in release['assets'] if item['name'] == args.asset.name]
    if len(matches) > 1:
        raise RuntimeError('Ambiguous existing release assets.')
    if matches:
        asset = matches[0]
        if asset['size'] != size or asset.get('digest') != f'sha256:{digest}':
            raise RuntimeError('An existing installer differs. Refusing to replace it.')
    else:
        if not release['draft']:
            raise RuntimeError('Refusing to add an unreviewed asset to an existing published release.')
        url = urllib.parse.urlsplit(release['upload_url'].split('{')[0])
        connection = http.client.HTTPSConnection(url.hostname, timeout=300)
        connection.putrequest('POST', f'{url.path}?{urllib.parse.urlencode({"name": args.asset.name})}')
        for name, value in {**headers, 'Content-Type': 'application/octet-stream', 'Content-Length': str(size)}.items():
            connection.putheader(name, value)
        connection.endheaders()
        with args.asset.open('rb') as source:
            while chunk := source.read(1024 * 1024):
                connection.send(chunk)
        response = connection.getresponse()
        payload = response.read()
        if response.status != 201:
            raise RuntimeError(f'Installer upload failed: HTTP {response.status}')
        asset = json.loads(payload)
        connection.close()
    if asset['size'] != size or asset.get('digest') != f'sha256:{digest}':
        raise RuntimeError('Uploaded installer checksum mismatch.')
    if release['draft']:
        release = api(f'/releases/{release["id"]}', {'body': notes, 'draft': False}, 'PATCH')
    verified = api(f'/releases/{release["id"]}')
    if verified['draft'] or verified['tag_name'] != args.tag:
        raise RuntimeError('Release publication could not be verified.')
    receipt = {
        'repository': args.repo, 'commit': commit, 'tag': args.tag,
        'url': verified['html_url'], 'publishedAt': verified['published_at'],
        'asset': {'name': asset['name'], 'bytes': size, 'sha256': digest, 'url': asset['browser_download_url']},
    }
    args.receipt.write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
