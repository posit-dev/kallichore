
## Kallichore

Kallichore is an experimental, headless supervisor for Jupyter kernels. 

It exposes a JSON API that can be used to start a kernel, send messages to and receive messages from the kernel, and stop the kernel. 

Multiple kernels/sessions can be supervised at once; each receives its own interface. 

```mermaid
graph LR
frontend -- http --> kallichore
kallichore -- sse --> frontend
kallichore --> ki1[kernel interface 1]
kallichore --> ki2[kernel interface 2]
ki1 -- zeromq --> k1[kernel 1]
ki2 -- zeromq --> k2[kernel 2]
```
