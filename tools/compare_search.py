#!/usr/bin/env python3
"""Alternate clean release probes; optionally evict only their residual pages.

Timings are public searches in synchronized bursts, not a sustained arrival
workload. Cache eviction runs outside the timers. No global purge or workload
control is used. JSONL retains every sample, provenance and cache observation.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import time


def residual_pages(paths, evict=False):
    """Inspect/evict clean file pages with a read-only mapping; never touch data.

    macOS msync(MS_SYNC | MS_INVALIDATE) drops cached pages. mincore verifies
    the outcome; a failed eviction must never be reported as a cold trial.
    This mapping belongs only to the external benchmark controller.
    """
    if sys.platform != 'darwin':
        raise RuntimeError('residual cache control requires macOS')
    lib = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
    lib.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int,
                         ctypes.c_int, ctypes.c_int, ctypes.c_longlong]
    lib.mmap.restype = ctypes.c_void_p
    lib.mincore.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
    lib.msync.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int]
    lib.munmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
    summary = dict(files=len(paths), bytes=0, pages=0, resident=0)
    for path in paths:
        with path.open('rb') as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_size == 0:
                raise ValueError('expected nonempty regular residual file')
            size = info.st_size
            pointer = lib.mmap(None, size, 1, 1, stream.fileno(), 0)
            if pointer == ctypes.c_void_p(-1).value:
                raise OSError(ctypes.get_errno(), 'mmap')
            try:
                if evict and lib.msync(pointer, size, 0x10 | 0x2):
                    raise OSError(ctypes.get_errno(), 'msync invalidate')
                pages = (size + os.sysconf('SC_PAGESIZE') - 1) // os.sysconf('SC_PAGESIZE')
                flags = (ctypes.c_ubyte * pages)()
                if lib.mincore(pointer, size, flags):
                    raise OSError(ctypes.get_errno(), 'mincore')
                summary['bytes'] += size
                summary['pages'] += pages
                summary['resident'] += sum(bool(flag & 1) for flag in flags)
            finally:
                if lib.munmap(pointer, size):
                    raise OSError(ctypes.get_errno(), 'munmap')
    if evict and summary['resident']:
        raise RuntimeError(f'residual cache eviction incomplete: {summary}')
    return summary


def retained_residuals(pid, directory, segments):
    output = subprocess.check_output(['/usr/sbin/lsof', '-a', '-p', str(pid), '-Fn'], text=True)
    paths = sorted({Path(line[1:]).resolve() for line in output.splitlines()
                    if line.startswith('n') and line.endswith('.residual')})
    if len(paths) != segments or any(path.parent != directory.resolve() for path in paths):
        raise RuntimeError('retained residual inventory does not match index')
    return paths


class Probe:
    def __init__(self, root, args, reuse):
        self.root = root.resolve()
        binary = self.root / 'target/release/spherra-bench'
        self.binary_sha256 = hashlib.sha256(binary.read_bytes()).hexdigest()
        command = [str(binary), 'search-probe', '--index-dir', str(args.index.resolve()),
                   '--rows', str(args.rows), '--queries', '256', '--seed', '20260804',
                   '--training-rows', str(args.training_rows), '--reuse', str(reuse).lower()]
        self.process = subprocess.Popen(command, cwd=self.root, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, text=True, bufsize=1)
        try:
            self.ready = self.read()
            if self.ready['kind'] != 'ready' or self.ready['revision']['dirty']:
                raise RuntimeError('benchmark requires a clean source revision')
            if self.ready['machine']['cargo_profile'] != 'release':
                raise RuntimeError('benchmark requires release binary')
        except BaseException:
            self.process.stdin.close()
            self.process.wait()
            raise

    def read(self):
        line = self.process.stdout.readline()
        if not line:
            raise RuntimeError(f'probe ended unexpectedly: {self.process.poll()}')
        return json.loads(line)

    def search(self, metric, queries):
        self.process.stdin.write(json.dumps(dict(metric=metric, queries=queries)) + '\n')
        self.process.stdin.flush()
        value = self.read()
        if value['kind'] != 'batch' or [r['query'] for r in value['results']] != queries:
            raise RuntimeError('unexpected probe reply')
        return value

    def close(self):
        self.process.stdin.close()
        end = self.read()
        code = self.process.wait()
        if code or end['kind'] != 'end' or end['revision'] != self.ready['revision']:
            raise RuntimeError('source changed or probe failed')
        return end


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', required=True, type=Path)
    parser.add_argument('--optimized', required=True, type=Path)
    parser.add_argument('--index', required=True, type=Path)
    parser.add_argument('--rows', required=True, type=int)
    parser.add_argument('--training-rows', type=int, default=4096)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--batches', type=int, default=32)
    parser.add_argument('--widths', type=int, nargs='+', default=[1, 2, 4, 8])
    parser.add_argument('--cold', action='store_true')
    args = parser.parse_args()
    if args.batches < 1 or any(w not in [1, 2, 4, 8] for w in args.widths):
        parser.error('positive batches and widths 1, 2, 4, 8 required')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    probes = {}
    with args.output.open('x') as output:
        def record(value):
            output.write(json.dumps(value, sort_keys=True) + '\n')
            output.flush()
        try:
            # First process may build an absent small index; both then open the
            # exact same immutable files. Existing indexes always require reuse.
            probes['optimized'] = Probe(args.optimized, args, args.index.exists())
            probes['baseline'] = Probe(args.baseline, args, True)
            for field in ['source', 'build', 'query_hash', 'query_count', 'model', 'rows', 'generation', 'segment_count']:
                if probes['baseline'].ready[field] != probes['optimized'].ready[field]:
                    raise RuntimeError(f'paired provenance mismatch: {field}')
            if probes['optimized'].ready['build']['dirty_worktree']:
                raise RuntimeError('index build provenance is dirty')
            record(dict(kind='provenance', timestamp=time.time(), arguments=vars(args) | {
                k: str(v) for k, v in vars(args).items() if isinstance(v, Path)},
                probes={k: dict(ready=p.ready, binary_sha256=p.binary_sha256) for k, p in probes.items()},
                load=os.getloadavg()))
            paths = None
            if args.cold:
                inventories = [retained_residuals(p.ready['pid'], args.index, p.ready['segment_count']) for p in probes.values()]
                if inventories[0] != inventories[1]:
                    raise RuntimeError('paired residual inventory mismatch')
                paths = inventories[0]
                record(dict(kind='residual_files', paths=[str(path) for path in paths]))
            fingerprints = {}
            for metric in ['cosine', 'dot']:
                for width in args.widths:
                    for p in probes.values():
                        for _ in range(3):
                            p.search(metric, list(range(248, 248 + width)))
                    for cache in (['warm', 'cold-residual'] if args.cold else ['warm']):
                        for batch in range(args.batches):
                            ids = [(batch * width + i) % 248 for i in range(width)]
                            order = ['baseline', 'optimized'] if batch % 2 == 0 else ['optimized', 'baseline']
                            trial = dict(kind='pair', metric=metric, width=width, cache=cache,
                                         batch=batch, queries=ids, order=order, load=os.getloadavg(), samples={})
                            for name in order:
                                cache_info = None
                                if paths:
                                    before = residual_pages(paths)
                                    if cache == 'cold-residual':
                                        cache_info = dict(before=before, after_eviction=residual_pages(paths, evict=True))
                                    else:
                                        # Prime this exact request so warm means its candidate pages are warm.
                                        probes[name].search(metric, ids)
                                value = probes[name].search(metric, ids)
                                if cache_info:
                                    cache_info['after_search'] = residual_pages(paths)
                                    value['cache'] = cache_info
                                for result in value['results']:
                                    key = (metric, result['query'])
                                    previous = fingerprints.setdefault(key, result['fingerprint'])
                                    if previous != result['fingerprint']:
                                        raise RuntimeError(f'result mismatch: {name}, {key}')
                                trial['samples'][name] = value
                            record(trial)
                        print(f'{args.rows} rows {metric} {width} callers {cache}: {args.batches} pairs', flush=True)
            for name, probe in probes.items():
                record(dict(kind='end', name=name, result=probe.close(), load=os.getloadavg()))
        finally:
            for probe in probes.values():
                if probe.process.poll() is None:
                    probe.process.stdin.close()
                    probe.process.wait()


if __name__ == '__main__':
    main()
