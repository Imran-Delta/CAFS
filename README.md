# Configurable Adaptive FileSystem (CAFS)
## Source Code

A FUSE‑based filesystem.
Kernel Driver not recommended.

## About Origins
Hoi, so the origins is me (Imran) wanting to go all in into one hobby project. First!:
```
Disclaimer: AI WAS USED! It's outputs are checked thoroughly!
```
Basically, I take almost everything in [Docs](Docs/) and use Claude to make the code, then have Deepseek and Gemini check the code, then I read it myself fully! So I hope no bugs go through.
```
But I am open to bugs and advice!!!!
```
~~The main idea was, a filesystem that can be configured, used extremely and ussd on semi-reliable devices.~~
* The main idea is bringing customizability to consumer drives, with as much features as possible, and adaptability for almost all hardware.
### Note:
**If your drive is failing. No amount of software can help fix it, but (the software) can try to delay the loss of data.**

## Status

### Alpha. Not production‑ready.
1. S.M.A.R.T. Handler done, but needs update once allocator is being worked on.
2. I/O Engine V1 Demo is done, in rust to prevent AI Hallucination to happen to a certain extent.

* Next I will possibly work on the file strcuture, allocator or tests (To test on real hardware partitions)

## Building
### ;-; IGNORE the make please. I need to research rust.
```bash
make
```

## Documentation

1. See **[Docs](Docs/)** for the on‑disk format specification and configuration reference.
2. See **[Src](Src/)** for the build source code.
3. For Current Structure Draft (AI), see [CAFS Repo Setup](cafs-repo-structure-2026-09-03.md).

## License

Multiple. See [LICENSE](LICENSE).
