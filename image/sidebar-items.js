initSidebarItems({"enum":[["ColorType","An enumeration over supported color types and their bit depths"],["DynamicImage","A Dynamic Image"],["FilterType","Available Sampling Filters"],["ImageError","An enumeration of Image Errors"],["ImageFormat","An enumeration of supported image formats. Not all formats support both encoding and decoding."]],"struct":[["Frame","A single animation frame"],["Frames","Hold the frames of the animated image"],["ImageBuffer","Generic image buffer"],["Luma","Grayscale colors"],["LumaA","Grayscale colors + alpha channel"],["MutPixels","Mutable pixel iterator DEPRECATED: It is currently not possible to create a safe iterator for this in Rust. You have to use an iterator over the image buffer instead."],["Pixels","Immutable pixel iterator"],["Rgb","RGB colors"],["Rgba","RGB colors + alpha channel"],["SubImage","A View into another image"]],"trait":[["GenericImage","A trait for manipulating images."],["ImageDecoder","The trait that all decoders implement"],["Pixel","A generalized pixel."],["Primitive","Primitive trait from old stdlib, added max_value"]],"fn":[["load","Create a new image from a Reader"],["load_from_memory","Create a new image from a byte slice Makes an educated guess about the image format. TGA is not supported by this function."],["load_from_memory_with_format","Create a new image from a byte slice"],["open","Open the image located at the path specified. The image's format is determined from the path's file extension."],["save_buffer","Saves the supplied buffer to a file at the path specified."]],"type":[["GrayAlphaImage","Sendable grayscale + alpha channel image buffer"],["GrayImage","Sendable grayscale image buffer"],["ImageResult","Result of an image decoding/encoding process"],["RgbImage","Sendable Rgb image buffer"],["RgbaImage","Sendable Rgb + alpha channel image buffer"]],"mod":[["bmp","Decoding of BMP Images"],["gif","Decoding of GIF Images"],["imageops","Image Processing Functions"],["jpeg","Decoding and Encoding of JPEG Images"],["math","Mathematical helper functions and types."],["png","Decoding and Encoding of PNG Images"],["ppm","Encoding of portable pixmap Images"],["tga","Decoding of TGA Images"],["tiff","Decoding and Encoding of TIFF Images"],["webp","Decoding of Webp Images"]]});