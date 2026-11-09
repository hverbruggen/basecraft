"""Zenodo's documented deposit API; no automatic retries or publication calls.

Credentials enter only through the runtime environment, never operation parameters.
Transport is injectable for tests. Only fixed HTTPS origins may receive credentials.
"""
import hashlib
import http.client
import json
import os
from pathlib import Path
from urllib.parse import quote, urlsplit

from basecraft.plugin import Plugin

plugin = Plugin("basecraft.zenodo", "0.1.0")
ORIGINS = {"sandbox": "https://sandbox.zenodo.org", "production": "https://zenodo.org"}
MAX_RESPONSE = 2 * 1024 * 1024
MAX_BYTES = 50 * 1000**3


class Cancelled(Exception):
    pass


class RemoteError(Exception):
    pass


def save(path, data):
    """Persist remote uncertainty/identity before proceeding to another request."""
    path = Path(path)
    temporary = path.with_suffix(".tmp")
    try:
        with temporary.open("w", encoding="utf-8") as stream:
            json.dump(data, stream, ensure_ascii=False, allow_nan=False)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
        if os.name != "nt":
            descriptor = os.open(path.parent, os.O_RDONLY)
            try:
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
    finally:
        temporary.unlink(missing_ok=True)


def check_cancel(context):
    if Path(context["cancel_file"]).exists():
        raise Cancelled()


def stage(context, text):
    check_cancel(context)
    # Same line-oriented stage file used by the shared task UI.
    Path(context["progress_file"]).write_text(text + "\n", encoding="utf-8")


def digest_file(path, context):
    sha = hashlib.sha256()
    md5 = hashlib.md5(usedforsecurity=False)  # Zenodo's object integrity field.
    size = 0
    with Path(path).open("rb") as stream:
        while block := stream.read(1024 * 1024):
            check_cancel(context)
            size += len(block)
            if size > MAX_BYTES:
                raise ValueError("A selected file exceeds Zenodo's 50 GB limit")
            sha.update(block)
            md5.update(block)
    return {"size": size, "sha256": sha.hexdigest(), "md5": md5.hexdigest()}


def selection_id(files):
    identities = [{k: file[k] for k in ("name", "size", "sha256", "md5")} for file in files]
    return hashlib.sha256(json.dumps(identities, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def valid_name(name):
    return (isinstance(name, str) and name not in ("", ".", "..") and len(name.encode()) <= 255
            and not any(char in name for char in "/\\\r\n\0") and not any(ord(c) < 32 for c in name))


def prepare(context):
    paths = context["inputs"]["files"]
    evidence = context["input_evidence"]["files"]
    if not 1 <= len(paths) <= 100 or len(evidence) != len(paths):
        raise ValueError("Select between 1 and 100 files")
    files = []
    for index, (path, source) in enumerate(zip(paths, evidence)):
        stage(context, f"Preparing file {index + 1} of {len(paths)}")
        name = Path(source.get("original_path") or path).name
        if not valid_name(name) or any(file["name"] == name for file in files):
            raise ValueError("Selected files need distinct, valid filenames")
        files.append({"name": name, "snapshot": path, **digest_file(path, context)})
    if sum(file["size"] for file in files) > MAX_BYTES:
        raise ValueError("Selected files exceed Zenodo's 50 GB record limit")
    plan = {"schema": 1, "selection": selection_id(files), "files": files,
            "total_bytes": sum(file["size"] for file in files)}
    save(context["outputs"]["receipt"], plan)
    return result(plan)


def metadata(parameters):
    title = parameters.get("title", "").strip()
    description = parameters.get("description", "").strip()
    creators = [name.strip() for name in parameters.get("creators", "").splitlines() if name.strip()]
    license_id = parameters.get("license", "").strip()
    if not title or len(title) > 1000:
        raise ValueError("Enter a title of 1–1000 characters")
    if not description or len(description) > 10000:
        raise ValueError("Enter a description of 1–10000 characters")
    if not creators or len(creators) > 100 or any(len(name) > 255 for name in creators):
        raise ValueError("Enter 1–100 creator names, one per line (Family name, Given name)")
    if not license_id or len(license_id) > 100 or not all(c.isalnum() or c in "-._" for c in license_id):
        raise ValueError("Enter a Zenodo license identifier, for example cc-by-4.0")
    access = parameters.get("access_right", "open")
    if access not in ("open", "closed"):
        raise ValueError("Only open or closed file access is supported")
    data = {"title": title, "description": description, "creators": [{"name": n} for n in creators],
            "upload_type": "dataset", "access_right": access}
    if access == "open":
        data["license"] = license_id
    return data


def safe_url(url, origin, prefix):
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or parsed.netloc != urlsplit(origin).netloc
            or parsed.query or parsed.fragment or not parsed.path.startswith(prefix)
            or "/../" in parsed.path):
        raise RemoteError("Zenodo returned an unexpected URL; no credentials sent")
    return url


def request(method, url, token, context, *, body=None, file=None, size=None):
    """Raw object PUT, not multipart. Never follow redirects or expose bodies/errors."""
    parsed = urlsplit(url)
    if parsed.scheme != "https" or parsed.netloc not in ("sandbox.zenodo.org", "zenodo.org"):
        raise RemoteError("Unsupported Zenodo origin")
    headers = {"Authorization": "Bearer " + token, "User-Agent": "Basecraft/0.1 (Zenodo draft submission)"}
    payload = json.dumps(body).encode() if body is not None else b""
    headers["Content-Type"] = "application/octet-stream" if file else "application/json"
    headers["Content-Length"] = str(size if file else len(payload))
    connection = http.client.HTTPSConnection(parsed.netloc, timeout=30)
    try:
        check_cancel(context)
        connection.putrequest(method, parsed.path)
        for key, value in headers.items():
            connection.putheader(key, value)
        connection.endheaders()
        if file:
            with Path(file).open("rb") as stream:
                while block := stream.read(1024 * 1024):
                    check_cancel(context)
                    connection.send(block)
        elif payload:
            connection.send(payload)
        check_cancel(context)
        response = connection.getresponse()
        raw = response.read(MAX_RESPONSE + 1)
        if response.status in (401, 403):
            raise RemoteError("Authentication expired or lacks deposit:write permission; enter a new token")
        if not 200 <= response.status < 300:
            raise RemoteError(f"Zenodo HTTP {response.status}; inspect the retained draft before retrying")
        if len(raw) > MAX_RESPONSE:
            raise RemoteError("Zenodo response exceeds the supported limit")
        return json.loads(raw)
    finally:
        connection.close()


def result(data):
    return {"ops": [], "data": data, "artifacts": [{"role": "receipt", "path": "."}]}


def upload(parameters, context, transport=request, token=None):
    # Never accept credentials or arbitrary endpoints from parameters.
    meta = metadata(parameters)
    origin = ORIGINS[parameters.get("service", "sandbox")]
    token = os.environ.get("BASECRAFT_ZENODO_TOKEN", "") if token is None else token
    if not token or len(token) > 8192 or any(c in token for c in "\r\n\0"):
        raise ValueError("Enter a runtime Zenodo access token (not saved)")
    plan = json.loads(Path(context["inputs"]["selection"][0]).read_text())
    paths = context["inputs"]["files"]
    if (plan.get("schema") != 1 or not 1 <= len(plan["files"]) <= 100
            or len(paths) != len(plan["files"]) or selection_id(plan["files"]) != plan["selection"]):
        raise ValueError("Selection preview is invalid; prepare the files again")
    for index, (path, expected) in enumerate(zip(paths, plan["files"])):
        stage(context, f"Verifying snapshot {index + 1} of {len(paths)}")
        if not valid_name(expected["name"]) or digest_file(path, context) != {k: expected[k] for k in ("size", "sha256", "md5")}:
            raise ValueError("Selected bytes changed after preview; prepare again")
    if len({f["name"] for f in plan["files"]}) != len(paths) or sum(f["size"] for f in plan["files"]) > MAX_BYTES:
        raise ValueError("Invalid duplicate names or total size in selection preview")
    receipt = {"schema": 1, "status": "not_sent", "service": parameters.get("service", "sandbox"),
               "selection": plan["selection"], "metadata": meta, "draft_id": None, "url": "",
               "files": [{k: f[k] for k in ("name", "size", "sha256", "md5")} for f in plan["files"]],
               "uploaded": [], "message": "No request sent"}
    previous = context["inputs"].get("resume", [])
    if previous:
        saved = json.loads(Path(previous[0]).read_text())
        if any(saved.get(key) != receipt[key] for key in ("schema", "service", "selection", "metadata", "files")):
            raise ValueError("Retry must use the exact previous selection, service and metadata")
        if not isinstance(saved.get("draft_id"), int) or saved["draft_id"] <= 0:
            raise ValueError("Creation status unknown: inspect your Zenodo uploads; do not create another draft automatically")
        receipt["draft_id"] = saved["draft_id"]
        receipt["url"] = f"{origin}/uploads/{saved['draft_id']}"
    output = context["outputs"]["receipt"]
    save(output, receipt)
    try:
        check_cancel(context)
        if receipt["draft_id"] is None:
            stage(context, "Creating Zenodo draft")
            receipt.update(status="creation_unknown", message="Creation may have reached Zenodo; do not repeat automatically")
            save(output, receipt)
            remote = transport("POST", origin + "/api/deposit/depositions", token, context, body={"metadata": meta})
            identifier = remote.get("id")
            if type(identifier) is not int or identifier <= 0:
                raise RemoteError("Zenodo did not return a valid draft identity")
            receipt.update(draft_id=identifier, url=f"{origin}/uploads/{identifier}", status="draft_partial",
                           message="Draft created; uploading files")
            save(output, receipt)  # Retain identity before inspecting any other response field.
        else:
            stage(context, "Checking retained Zenodo draft")
            remote = transport("GET", f"{origin}/api/deposit/depositions/{receipt['draft_id']}", token, context)
        if remote.get("id") != receipt["draft_id"] or remote.get("submitted") is not False:
            raise RemoteError("The retained deposit is no longer an editable draft; nothing overwritten")
        # Do not overwrite another editor's metadata during resume.
        actual = remote.get("metadata", {})
        if any(actual.get(key) != value for key, value in meta.items()):
            raise RemoteError("Remote draft metadata changed; review it on Zenodo before retrying")
        bucket = safe_url(remote["links"]["bucket"], origin, "/api/files/")
        existing = {file["filename"]: file for file in remote.get("files", [])}
        if set(existing) - {file["name"] for file in plan["files"]}:
            raise RemoteError("Remote draft contains additional files; review it on Zenodo")
        for index, (path, file) in enumerate(zip(paths, plan["files"])):
            stage(context, f"Uploading file {index + 1} of {len(paths)}: {file['name']}")
            known = existing.get(file["name"])
            if known is not None:
                checksum = known.get("checksum", "").removeprefix("md5:")
                if checksum != file["md5"] or known.get("filesize") != file["size"]:
                    raise RemoteError("A remote filename has different bytes; automatic overwrite refused")
            else:
                receipt.update(status="upload_unknown", message="Upload may be partial; retry only this retained draft")
                save(output, receipt)
                uploaded = transport("PUT", bucket.rstrip("/") + "/" + quote(file["name"], safe=""), token, context,
                                     file=path, size=file["size"])
                if (uploaded.get("checksum") != "md5:" + file["md5"] or uploaded.get("size") != file["size"]
                        or uploaded.get("key") != file["name"]):
                    raise RemoteError("Zenodo upload checksum/name/size differs; inspect the draft before retrying")
            receipt["uploaded"].append(file["name"])
            receipt.update(status="draft_partial", message="Draft retained; remaining files can be retried")
            save(output, receipt)
        check_cancel(context)
        receipt.update(status="draft_ready", message="Draft uploaded. It is not published and has no published DOI. Review and publish on Zenodo.")
    except Cancelled:
        receipt["message"] = "Cancelled. Remote changes remain; inspect this receipt before retrying."
        if receipt["status"] not in ("creation_unknown", "upload_unknown"):
            receipt["status"] = "cancelled"
    except RemoteError as error:
        # These messages are constructed locally from safe status codes, never
        # service response text, URLs or credential-bearing exception messages.
        receipt["message"] = str(error)
        if receipt["status"] not in ("creation_unknown", "upload_unknown"):
            receipt["status"] = "draft_partial" if receipt["draft_id"] else "not_sent"
    except (OSError, ValueError, KeyError, TypeError, http.client.HTTPException):
        # Exception/response text can contain credentials or user-controlled HTML.
        # Keep the structured identity, but never persist transport exception bodies.
        receipt["message"] = "Zenodo request failed or response was invalid. Check authentication, connectivity and the retained draft before retrying."
        if receipt["status"] not in ("creation_unknown", "upload_unknown"):
            receipt["status"] = "draft_partial" if receipt["draft_id"] else "not_sent"
    save(output, receipt)
    return result(receipt)


@plugin.context_operation("prepare")
def prepare_operation(_snapshot, _parameters, context):
    return prepare(context)


@plugin.context_operation("draft")
def draft_operation(_snapshot, parameters, context):
    return upload(parameters, context)


if __name__ == "__main__":
    plugin.run()
