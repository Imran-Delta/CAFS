# Configurable Adaptive FileSystem (CAFS)
## Source Code

A FUSE‑based filesystem.
Kernel Driver not recommended.

## About Origins
Hoi, so the origins is me (Imran) wanting to go all in into one hobby project. First!:
```
Disclaimer: AI WAS USED! It's outputs are checked thoroughly!
```
Basically, I take the [Docs](Docs/) and use Claude to make the code, then have Deepseek and Gemini check the code, then I read it myself fully! So I hope no bugs go through.
But I am open to bugs and advice!!!!
The main idea was, a filesystem that can be configured, bashed and ussd on semi-reliable devices.
### Note:
**If your drive is failing. No amount of software can help fix it, but (the software) can try to delay the loss of data.**

## Status

Alpha. Not production‑ready.
S.M.A.R.T. Handler almost done.

## Building

```bash
make
```

## Documentation

1. See **[Docs](Docs/)** for the on‑disk format specification and configuration reference.
2. See **[Src](Src/)** for the build source code.
3. For Current Structure Draft (AI), see [CAFS Repo Setup](cafs-repo-structure-2026-08-29.md).

## License

BSD 3‑Clause. See [LICENSE](LICENSE).
