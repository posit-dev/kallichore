first, read CLAUDE.md for some context on this project

note that there are currently two ways to connect to the server

- TCP/HTTP: in this mode, the server listens for HTTP requests on a TCP port, and individual kernels can be connected to via channels-upgrade which creates a websocket connection for just that kernel
- Unix Domain Sockets: in this mode, the server listens for HTTP requests on a domain socket, and individual kernels can be connected to via channels-upgrade which creates a domain socket for just that kernel

your job is to add a THIRD way to connect to the server using named pipes on Windows

your solution must be written so that the first two ways still work. try not to touch the code for those. the code must still compile and run on linux and macos, so any Windows code you add needs a cfg guard

named pipe mode is the Windows alternative to domain sockets, and it should work the same way

- there should be an argument giving the path to the named pipe when starting the server; if this argument is provided, the server should accept requests on the named pipe instead of starting a tcp listener
- the channels-upgrade request should create and return the path to a named pipe, and then use it to emit kernel events (just like the websocket and the domain socket codepaths)

when you are confident in your approach, add an integration test for it
