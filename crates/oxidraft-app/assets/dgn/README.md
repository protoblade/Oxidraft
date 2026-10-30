# DGN V8 seed drawing

`seed_v8.dgn` is GDAL's DGN V8 test file, from its repository
(<https://github.com/OSGeo/gdal>, `autotest/ogr/data/dgnv8/test_dgnv8.dgn`),
under GDAL's license (MIT/X style; `LICENSE.GDAL.TXT`, GDAL's own). Its contents
are as GDAL's `test_dgnv8_ref.csv` lists them: a model `my_model` of points,
lines, line strings, text, point strings, ellipses, arcs, curves, B-spline
curves, complex chains and shapes, cells and a shared cell, on level 64
(`Default`).

DGNIMPORT's V8 reader is tested against it, and DGNEXPORT writes V8 files on
it: its header, settings, level table and the rest kept, its model's
elements replaced by the drawing's, a level added for each layer.
