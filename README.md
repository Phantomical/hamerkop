# Hamerkop - Experimental caching server based off of io_uring

This is an experiment in writing the networking part of a cache server
using io_uring to run the needed network calls asynchronously. The main
parts are composed of two projects:
1. `uring` - A binding to the io_uring APIs, and
2. `hamerkop` - An async executor built on top of `uring`.

This is meant more as an exploratory playground to figure out how to
make use this API to build the networking component of the cache server
while maintaining a consistently low latency the entire time.

Since this repo is meant to be experimentation around writing a cache
server there are also experiments in writing some of the other components
that are part of a well-functioning cache:
1. `metrics` - A distributed metrics registration crate that allows metrics
   to be created declare anywhere as globals (using the appropriate
   registration macro) and then accessed as a global list when needed.
2. `tokenreplace` - A find-and-replace macro that runs after other attribute
   macros have been expanded.

## Examples 
`hamerkop/examples` contains two examples showing how the library could be
used. These are respectively a pingserver and an echo server. They were
developed on linux 5.10 but may work a few versions earlier than that.

To run them do one of
```bash
cargo run --examples echoserver
cargo run --examples pingserver
```
and then the respective server will start listening on port 8000.
