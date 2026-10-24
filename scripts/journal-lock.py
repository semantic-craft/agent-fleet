#!/usr/bin/env python3
"""POSIX session lock held until the owning Node process closes stdin. Stdlib only."""
import fcntl
import json
import os
import stat
import sys


def emit(state, **fields):
    print(json.dumps(dict(state=state, **fields)), flush=True)


def main():
    directory, filename = sys.argv[1:]
    root = os.lstat(directory)
    if not stat.S_ISDIR(root.st_mode) or root.st_uid != os.getuid() or root.st_mode & 0o077:
        raise ValueError('unsafe directory')
    fd = os.open(os.path.join(directory, filename), os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1 or info.st_mode & 0o077:
            raise ValueError('unsafe lock')
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            emit('busy')
            return
        def identity(st):
            return str(st.st_dev) + ':' + str(st.st_ino)

        def check_identity():
            current_root = os.lstat(directory)
            current_lock = os.lstat(os.path.join(directory, filename))
            current_fd = os.fstat(fd)
            if (not stat.S_ISDIR(current_root.st_mode) or identity(current_root) != identity(root)
                    or not stat.S_ISREG(current_lock.st_mode) or identity(current_lock) != identity(info)
                    or current_lock.st_nlink != 1 or current_fd.st_nlink != 1):
                raise ValueError('lock identity lost')

        check_identity()
        emit('locked', rootIdentity=identity(root), lockIdentity=identity(info))
        for line in sys.stdin:
            msg = json.loads(line)
            if msg.get('op') != 'check' or not isinstance(msg.get('nonce'), str):
                raise ValueError('invalid command')
            check_identity()
            emit('held', nonce=msg['nonce'])
    finally:
        os.close(fd)


if __name__ == '__main__':
    try:
        main()
    except Exception:
        # No filenames, environment, exception text or caller data in diagnostics.
        emit('unavailable')
        sys.exit(1)
