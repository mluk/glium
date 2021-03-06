use std::borrow::Borrow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem;

use Handle;
use buffer::BufferViewAnySlice;
use program::Program;
use vertex::AttributeType;
use vertex::VertexFormat;
use GlObject;
use BufferViewExt;

use {libc, gl};
use context::CommandContext;
use version::Api;
use version::Version;

/// Stores and handles vertex attributes.
pub struct VertexAttributesSystem {
    // we maintain a list of VAOs for each vertexbuffer-indexbuffer-program association
    // the key is a (buffers-list-with-offset, program) ; the buffers list must be sorted
    vaos: RefCell<HashMap<(Vec<(gl::types::GLuint, usize)>, Handle), VertexArrayObject>>,
}

/// Object allowing one to bind vertex attributes to the current context.
pub struct Binder<'a, 'c, 'd: 'c> {
    context: &'c mut CommandContext<'d>,
    program: &'a Program,
    element_array_buffer: gl::types::GLuint,
    vertex_buffers: Vec<(gl::types::GLuint, VertexFormat, usize, usize, Option<u32>)>,
}

impl VertexAttributesSystem {
    /// Builds a new `VertexAttributesSystem`.
    pub fn new() -> VertexAttributesSystem {
        VertexAttributesSystem {
            vaos: RefCell::new(HashMap::new()),
        }
    }

    /// Starts the process of binding vertex attributes.
    pub fn start<'a, 'c, 'd>(ctxt: &'c mut CommandContext<'d>, program: &'a Program,
                             indices: gl::types::GLuint) -> Binder<'a, 'c, 'd>
    {
        Binder {
            context: ctxt,
            program: program,
            element_array_buffer: indices,
            vertex_buffers: Vec::with_capacity(1),
        }
    }

    /// This function *must* be called whenever you destroy a buffer so that the system can
    /// purge its VAOs cache.
    pub fn purge_buffer(ctxt: &mut CommandContext, id: gl::types::GLuint) {
        VertexAttributesSystem::purge_if(ctxt, |&(ref buffers, _)| {
            buffers.iter().find(|&&(b, _)| b == id).is_some()
        })
    }

    /// This function *must* be called whenever you destroy a program so that the system can
    /// purge its VAOs cache.
    pub fn purge_program(ctxt: &mut CommandContext, program: Handle) {
        VertexAttributesSystem::purge_if(ctxt, |&(_, p)| p == program)
    }

    /// Purges the VAOs cache.
    pub fn purge_all(ctxt: &mut CommandContext) {
        let vaos = mem::replace(&mut *ctxt.vertex_array_objects.vaos.borrow_mut(),
                                HashMap::new());

        for (_, vao) in vaos {
            vao.destroy(ctxt);
        }
    }

    /// Purges the VAOs cache. Contrary to `purge_all`, this function expects the system to be
    /// destroyed soon.
    pub fn cleanup(ctxt: &mut CommandContext) {
        let vaos = mem::replace(&mut *ctxt.vertex_array_objects.vaos.borrow_mut(),
                                HashMap::with_capacity(0));

        for (_, vao) in vaos {
            vao.destroy(ctxt);
        }
    }

    /// Tells the VAOs system that the currently binded element array buffer will change.
    pub fn hijack_current_element_array_buffer(ctxt: &mut CommandContext) {
        let vaos = ctxt.vertex_array_objects.vaos.borrow_mut();

        for (_, vao) in vaos.iter() {
            if vao.id == ctxt.state.vertex_array {
                vao.element_array_buffer_hijacked.set(true);
                return;
            }
        }
    }

    /// Purges VAOs that match a certain condition.
    fn purge_if<F>(ctxt: &mut CommandContext, mut condition: F)
                   where F: FnMut(&(Vec<(gl::types::GLuint, usize)>, Handle)) -> bool
    {
        let mut vaos = ctxt.vertex_array_objects.vaos.borrow_mut();

        let mut keys = Vec::with_capacity(4);
        for (key, _) in &*vaos {
            if condition(key) {
                keys.push(key.clone());
            }
        }

        for key in keys {
            vaos.remove(&key).unwrap().destroy(ctxt);
        }
    }
}

impl<'a, 'c, 'd: 'c> Binder<'a, 'c, 'd> {
    /// Adds a buffer to bind as a source of vertices.
    ///
    /// # Parameters
    ///
    /// - `buffer`: The buffer to bind.
    /// - `first`: Offset of the first element of the buffer in number of elements.
    /// - `divisor`: If `Some`, use this value for `glVertexAttribDivisor` (instancing-related).
    pub fn add(mut self, buffer: &BufferViewAnySlice, bindings: &VertexFormat, divisor: Option<u32>)
               -> Binder<'a, 'c, 'd>
    {
        let offset = buffer.get_offset_bytes();
        let (buffer, format, stride) = (buffer.get_buffer_id(self.context), bindings.clone(),
                                        buffer.get_elements_size());

        self.vertex_buffers.push((buffer, format, offset, stride, divisor));
        self
    }

    /// Finish binding the vertex attributes.
    pub fn bind(self) {
        let ctxt = self.context;

        if ctxt.version >= &Version(Api::Gl, 3, 0) || ctxt.version >= &Version(Api::GlEs, 3, 0) ||
           ctxt.extensions.gl_arb_vertex_array_object || ctxt.extensions.gl_oes_vertex_array_object
           || ctxt.extensions.gl_apple_vertex_array_object
        {
            // VAOs are supported
            let mut buffers_list: Vec<_> = self.vertex_buffers.iter()
                                                              .map(|&(v, _, o, _, _)| (v, o))
                                                              .collect();
            buffers_list.push((self.element_array_buffer, 0));
            buffers_list.sort();

            let program_id = self.program.get_id();

            // trying to find an existing VAO in the cache
            if let Some(value) = ctxt.vertex_array_objects.vaos.borrow_mut()
                                     .get(&(buffers_list.clone(), program_id))
            {
                value.bind(ctxt);
                return;
            }

            // if not found, building a new one
            let new_vao = unsafe {
                VertexArrayObject::new(ctxt, &self.vertex_buffers,
                                       self.element_array_buffer, self.program)
            };

            new_vao.bind(ctxt);
            ctxt.vertex_array_objects.vaos.borrow_mut().insert((buffers_list, program_id), new_vao);

        } else {
            // VAOs are not supported

            // just in case
            if ctxt.state.vertex_array != 0 {
                bind_vao(ctxt, 0);
                ctxt.state.vertex_array = 0;
            }

            unsafe {
                if ctxt.version >= &Version(Api::Gl, 1, 5) ||
                    ctxt.version >= &Version(Api::GlEs, 2, 0)
                {
                    ctxt.gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.element_array_buffer);
                } else if ctxt.extensions.gl_arb_vertex_buffer_object {
                    ctxt.gl.BindBufferARB(gl::ELEMENT_ARRAY_BUFFER_ARB, self.element_array_buffer);
                } else {
                    unreachable!();
                }
            }

            for (vertex_buffer, bindings, offset, stride, divisor) in self.vertex_buffers {
                unsafe {
                    bind_attribute(ctxt, self.program, vertex_buffer, &bindings, offset, stride,
                                   divisor);
                }
            }
        }
    }
}

/// Stores informations about how to bind a vertex buffer, an index buffer and a program.
struct VertexArrayObject {
    id: gl::types::GLuint,
    destroyed: bool,
    element_array_buffer: gl::types::GLuint,
    element_array_buffer_hijacked: Cell<bool>,
}

impl VertexArrayObject {
    /// Builds a new `VertexArrayObject`.
    ///
    /// The vertex buffer, index buffer and program must not outlive the
    /// VAO, and the VB & program attributes must not change.
    unsafe fn new(mut ctxt: &mut CommandContext,
                  vertex_buffers: &[(gl::types::GLuint, VertexFormat, usize, usize, Option<u32>)],
                  ib_id: gl::types::GLuint, program: &Program) -> VertexArrayObject
    {
        // checking the attributes types
        for &(_, ref bindings, _, _, _) in vertex_buffers {
            for &(ref name, _, ty) in bindings.iter() {
                let attribute = match program.get_attribute(Borrow::<str>::borrow(name)) {
                    Some(a) => a,
                    None => continue
                };

                if ty.get_num_components() != attribute.ty.get_num_components() ||
                    attribute.size != 1
                {
                    panic!("The program attribute `{}` does not match the vertex format. \
                            Program expected {:?}, got {:?}.", name, attribute.ty, ty);
                }
            }
        }

        // checking for missing attributes
        for (&ref name, _) in program.attributes() {
            let mut found = false;
            for &(_, ref bindings, _, _, _) in vertex_buffers {
                if bindings.iter().find(|&&(ref n, _, _)| n == name).is_some() {
                    found = true;
                    break;
                }
            }
            if !found {
                panic!("The program attribute `{}` is missing in the vertex bindings", name);
            }
        };

        // TODO: check for collisions between the vertices sources

        // building the VAO
        let id = {
            let mut id = mem::uninitialized();
            if ctxt.version >= &Version(Api::Gl, 3, 0) ||
                ctxt.version >= &Version(Api::GlEs, 3, 0) ||
                ctxt.extensions.gl_arb_vertex_array_object
            {
                ctxt.gl.GenVertexArrays(1, &mut id);
            } else if ctxt.extensions.gl_oes_vertex_array_object {
                ctxt.gl.GenVertexArraysOES(1, &mut id);
            } else if ctxt.extensions.gl_apple_vertex_array_object {
                ctxt.gl.GenVertexArraysAPPLE(1, &mut id);
            } else {
                unreachable!();
            };
            id
        };

        // we don't use DSA as we're going to make multiple calls for this VAO
        // and we're likely going to use the VAO right after it's been created
        bind_vao(&mut ctxt, id);

        // binding index buffer
        // the ELEMENT_ARRAY_BUFFER is part of the state of the VAO
        // TODO: use a proper function
        if ctxt.version >= &Version(Api::Gl, 1, 5) ||
            ctxt.version >= &Version(Api::GlEs, 2, 0)
        {
            ctxt.gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ib_id);
        } else if ctxt.extensions.gl_arb_vertex_buffer_object {
            ctxt.gl.BindBufferARB(gl::ELEMENT_ARRAY_BUFFER_ARB, ib_id);
        } else {
            unreachable!();
        }

        for &(vertex_buffer, ref bindings, offset, stride, divisor) in vertex_buffers {
            bind_attribute(ctxt, program, vertex_buffer, bindings, offset, stride, divisor);
        }

        VertexArrayObject {
            id: id,
            destroyed: false,
            element_array_buffer: ib_id,
            element_array_buffer_hijacked: Cell::new(false),
        }
    }

    /// Sets this VAO as the current VAO.
    fn bind(&self, ctxt: &mut CommandContext) {
        unsafe {
            bind_vao(ctxt, self.id);

            if self.element_array_buffer_hijacked.get() {
                // TODO: use a proper function
                if ctxt.version >= &Version(Api::Gl, 1, 5) ||
                    ctxt.version >= &Version(Api::GlEs, 2, 0)
                {
                    ctxt.gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.element_array_buffer);
                } else if ctxt.extensions.gl_arb_vertex_buffer_object {
                    ctxt.gl.BindBufferARB(gl::ELEMENT_ARRAY_BUFFER_ARB, self.element_array_buffer);
                } else {
                    unreachable!();
                }

                self.element_array_buffer_hijacked.set(false);
            }
        }
    }

    /// Must be called to destroy the VAO (otherwise its destructor will panic as a safety
    /// measure).
    fn destroy(mut self, mut ctxt: &mut CommandContext) {
        self.destroyed = true;

        unsafe {
            // unbinding
            if ctxt.state.vertex_array == self.id {
                bind_vao(ctxt, 0);
                ctxt.state.vertex_array = 0;
            }

            // deleting
            if ctxt.version >= &Version(Api::Gl, 3, 0) ||
                ctxt.version >= &Version(Api::GlEs, 3, 0) ||
                ctxt.extensions.gl_arb_vertex_array_object
            {
                ctxt.gl.DeleteVertexArrays(1, [ self.id ].as_ptr());
            } else if ctxt.extensions.gl_oes_vertex_array_object {
                ctxt.gl.DeleteVertexArraysOES(1, [ self.id ].as_ptr());
            } else if ctxt.extensions.gl_apple_vertex_array_object {
                ctxt.gl.DeleteVertexArraysAPPLE(1, [ self.id ].as_ptr());
            } else {
                unreachable!();
            }
        }
    }
}

impl Drop for VertexArrayObject {
    fn drop(&mut self) {
        assert!(self.destroyed);
    }
}

impl GlObject for VertexArrayObject {
    type Id = gl::types::GLuint;

    fn get_id(&self) -> gl::types::GLuint {
        self.id
    }
}

fn vertex_binding_type_to_gl(ty: AttributeType) -> (gl::types::GLenum, gl::types::GLint) {
    match ty {
        AttributeType::I8 => (gl::BYTE, 1),
        AttributeType::I8I8 => (gl::BYTE, 2),
        AttributeType::I8I8I8 => (gl::BYTE, 3),
        AttributeType::I8I8I8I8 => (gl::BYTE, 4),
        AttributeType::U8 => (gl::UNSIGNED_BYTE, 1),
        AttributeType::U8U8 => (gl::UNSIGNED_BYTE, 2),
        AttributeType::U8U8U8 => (gl::UNSIGNED_BYTE, 3),
        AttributeType::U8U8U8U8 => (gl::UNSIGNED_BYTE, 4),
        AttributeType::I16 => (gl::SHORT, 1),
        AttributeType::I16I16 => (gl::SHORT, 2),
        AttributeType::I16I16I16 => (gl::SHORT, 3),
        AttributeType::I16I16I16I16 => (gl::SHORT, 4),
        AttributeType::U16 => (gl::UNSIGNED_SHORT, 1),
        AttributeType::U16U16 => (gl::UNSIGNED_SHORT, 2),
        AttributeType::U16U16U16 => (gl::UNSIGNED_SHORT, 3),
        AttributeType::U16U16U16U16 => (gl::UNSIGNED_SHORT, 4),
        AttributeType::I32 => (gl::INT, 1),
        AttributeType::I32I32 => (gl::INT, 2),
        AttributeType::I32I32I32 => (gl::INT, 3),
        AttributeType::I32I32I32I32 => (gl::INT, 4),
        AttributeType::U32 => (gl::UNSIGNED_INT, 1),
        AttributeType::U32U32 => (gl::UNSIGNED_INT, 2),
        AttributeType::U32U32U32 => (gl::UNSIGNED_INT, 3),
        AttributeType::U32U32U32U32 => (gl::UNSIGNED_INT, 4),
        AttributeType::F32 => (gl::FLOAT, 1),
        AttributeType::F32F32 => (gl::FLOAT, 2),
        AttributeType::F32F32F32 => (gl::FLOAT, 3),
        AttributeType::F32F32F32F32 => (gl::FLOAT, 4),
        AttributeType::F32x2x2 => (gl::FLOAT_MAT2, 1),
        AttributeType::F32x2x3 => (gl::FLOAT_MAT2x3, 1),
        AttributeType::F32x2x4 => (gl::FLOAT_MAT2x4, 1),
        AttributeType::F32x3x2 => (gl::FLOAT_MAT3x2, 1),
        AttributeType::F32x3x3 => (gl::FLOAT_MAT3, 1),
        AttributeType::F32x3x4 => (gl::FLOAT_MAT3x4, 1),
        AttributeType::F32x4x2 => (gl::FLOAT_MAT4x2, 1),
        AttributeType::F32x4x3 => (gl::FLOAT_MAT4x3, 1),
        AttributeType::F32x4x4 => (gl::FLOAT_MAT4, 1),
        AttributeType::F64 => (gl::DOUBLE, 1),
        AttributeType::F64F64 => (gl::DOUBLE, 2),
        AttributeType::F64F64F64 => (gl::DOUBLE, 3),
        AttributeType::F64F64F64F64 => (gl::DOUBLE, 4),
        AttributeType::F64x2x2 => (gl::DOUBLE_MAT2, 1),
        AttributeType::F64x2x3 => (gl::DOUBLE_MAT2x3, 1),
        AttributeType::F64x2x4 => (gl::DOUBLE_MAT2x4, 1),
        AttributeType::F64x3x2 => (gl::DOUBLE_MAT3x2, 1),
        AttributeType::F64x3x3 => (gl::DOUBLE_MAT3, 1),
        AttributeType::F64x3x4 => (gl::DOUBLE_MAT3x4, 1),
        AttributeType::F64x4x2 => (gl::DOUBLE_MAT4x2, 1),
        AttributeType::F64x4x3 => (gl::DOUBLE_MAT4x3, 1),
        AttributeType::F64x4x4 => (gl::DOUBLE_MAT4, 1),
    }
}

/// Binds the vertex array object as the current one. Unbinds if `0` is passed.
///
/// ## Panic
///
/// Panics if the backend doesn't support vertex array objects.
fn bind_vao(ctxt: &mut CommandContext, vao_id: gl::types::GLuint) {
    if ctxt.state.vertex_array != vao_id {
        if ctxt.version >= &Version(Api::Gl, 3, 0) ||
            ctxt.version >= &Version(Api::GlEs, 3, 0) ||
            ctxt.extensions.gl_arb_vertex_array_object
        {
            unsafe { ctxt.gl.BindVertexArray(vao_id) };
        } else if ctxt.extensions.gl_oes_vertex_array_object {
            unsafe { ctxt.gl.BindVertexArrayOES(vao_id) };
        } else if ctxt.extensions.gl_apple_vertex_array_object {
            unsafe { ctxt.gl.BindVertexArrayAPPLE(vao_id) };
        } else {
            unreachable!();
        }

        ctxt.state.vertex_array = vao_id;
    }
}

/// Binds an individual attribute to the current VAO.
unsafe fn bind_attribute(ctxt: &mut CommandContext, program: &Program,
                         vertex_buffer: gl::types::GLuint, bindings: &VertexFormat,
                         buffer_offset: usize, stride: usize, divisor: Option<u32>)
{
    // glVertexAttribPointer uses the current array buffer
    // TODO: use a proper function
    if ctxt.state.array_buffer_binding != vertex_buffer {
        if ctxt.version >= &Version(Api::Gl, 1, 5) ||
            ctxt.version >= &Version(Api::GlEs, 2, 0)
        {
            ctxt.gl.BindBuffer(gl::ARRAY_BUFFER, vertex_buffer);
        } else if ctxt.extensions.gl_arb_vertex_buffer_object {
            ctxt.gl.BindBufferARB(gl::ARRAY_BUFFER_ARB, vertex_buffer);
        } else {
            unreachable!();
        }
        ctxt.state.array_buffer_binding = vertex_buffer;
    }

    // binding attributes
    for &(ref name, offset, ty) in bindings.iter() {
        let (data_type, elements_count) = vertex_binding_type_to_gl(ty);

        let attribute = match program.get_attribute(Borrow::<str>::borrow(name)) {
            Some(a) => a,
            None => continue
        };

        let (attribute_ty, _) = vertex_binding_type_to_gl(attribute.ty);

        if attribute.location != -1 {
            match attribute_ty {
                gl::BYTE | gl::UNSIGNED_BYTE | gl::SHORT | gl::UNSIGNED_SHORT |
                gl::INT | gl::UNSIGNED_INT =>
                    ctxt.gl.VertexAttribIPointer(attribute.location as u32,
                                                 elements_count as gl::types::GLint, data_type,
                                                 stride as i32,
                                                 (buffer_offset + offset) as *const libc::c_void),

                gl::DOUBLE | gl::DOUBLE_VEC2 | gl::DOUBLE_VEC3 | gl::DOUBLE_VEC4 |
                gl::DOUBLE_MAT2 | gl::DOUBLE_MAT3 | gl::DOUBLE_MAT4 |
                gl::DOUBLE_MAT2x3 | gl::DOUBLE_MAT2x4 | gl::DOUBLE_MAT3x2 |
                gl::DOUBLE_MAT3x4 | gl::DOUBLE_MAT4x2 | gl::DOUBLE_MAT4x3 =>
                    ctxt.gl.VertexAttribLPointer(attribute.location as u32,
                                                 elements_count as gl::types::GLint, data_type,
                                                 stride as i32,
                                                 (buffer_offset + offset) as *const libc::c_void),

                _ => ctxt.gl.VertexAttribPointer(attribute.location as u32,
                                                 elements_count as gl::types::GLint, data_type, 0,
                                                 stride as i32,
                                                 (buffer_offset + offset) as *const libc::c_void)
            }

            if let Some(divisor) = divisor {
                ctxt.gl.VertexAttribDivisor(attribute.location as u32, divisor);
            }

            ctxt.gl.EnableVertexAttribArray(attribute.location as u32);
        }
    }
}
