# Manually run tests

First you need to build and start the echo-server:

```
docker build -t webtransport-echo-server echo-server
docker run -it --rm --network=host webtransport-echo-server
```

On another terminal run:

```
wasm-pack test --chrome
```

Navigate with your browser at http://127.0.0.1:8000.

You can also run the tests on a headless browser:

```
wasm-pack test --chrome --headless
```

> **Note:** For headless tests your Chrome browser needs to be compatible
> with chromedriver (i.e. they must have the same major version).
>
> You may need to define the path of chromedriver with `--chromedriver=/path/to/chromedriver`.
