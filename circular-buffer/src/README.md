# `circular-buffer`

An optimized FIFO queue that allows safe access to the internal storage as a slice (i.e. not
just element-by-element). This is useful for circular buffers of bytes. Since it uses
`smallvec`'s `Array` trait it can only be backed by an array of static size, this may change in
the future.
