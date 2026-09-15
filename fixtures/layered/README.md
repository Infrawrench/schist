# Layered raster interoperability fixtures

`paintnet-3.pdn` and `paintnet-4.pdn` are the unmodified
`oldPDN3510.pdn` and `Untitled3.pdn` samples from
[pypdn](https://github.com/addisonElliott/pypdn/tree/master/tests/data),
respectively. They were saved by Paint.NET 3.5.10 and 4.0.21 and contain
two bitmap layers. Their MIT license is included as `pypdn-LICENSE`.

The XCF fixtures were generated with GIMP 2.10.36 using `generate.scm`:

```sh
gimp -i -d -f --batch-interpreter=plug-in-script-fu-eval \
  -b '(load "fixtures/layered/generate.scm")' -b '(gimp-quit 0)'
```

`gimp-rle.xcf` has a 67×65 red background and a partially off-canvas blue
layer using Multiply and 50% opacity. It exercises the partial tiles at
the right and bottom edges and signed layer offsets. `gimp-group-16.xcf`
moves the blue layer into a group, attaches a disabled gray mask, and
converts to 16-bit precision before saving. Both are original test
artwork under the repository's MIT license.
