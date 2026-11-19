use egui::epaint::{ImageData, Primitive};
use egui::{
    ClippedPrimitive, TextureFilter, TextureId, TextureOptions, TextureWrapMode, TexturesDelta,
};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

type Device = *mut c_void;
type HResult = i32;

pub unsafe fn vfunc<T: Copy>(object: *mut c_void, index: usize) -> T {
    unsafe {
        let table = *(object as *const *const *const c_void);
        let address = *table.add(index);
        std::mem::transmute_copy(&address)
    }
}

pub unsafe fn release(object: *mut c_void) {
    unsafe {
        if !object.is_null() {
            let call: unsafe extern "system" fn(*mut c_void) -> u32 = vfunc(object, 2);
            call(object);
        }
    }
}

fn checked(result: HResult, operation: &str) -> Result<(), String> {
    if result < 0 {
        Err(format!("{operation}: 0x{:08X}", result as u32))
    } else {
        Ok(())
    }
}

#[repr(C)]
struct LockedRect {
    pitch: i32,
    bits: *mut c_void,
}

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct Viewport {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    min_z: f32,
    max_z: f32,
}

#[repr(C)]
struct Vertex {
    x: f32,
    y: f32,
    z: f32,
    rhw: f32,
    color: u32,
    u: f32,
    v: f32,
}

struct Texture {
    object: *mut c_void,
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    options: TextureOptions,
    dirty: bool,
}

impl Texture {
    unsafe fn invalidate(&mut self) {
        unsafe {
            release(self.object);
            self.object = null_mut();
            self.dirty = true;
        }
    }

    unsafe fn upload(&mut self, device: Device) -> Result<(), String> {
        unsafe {
            if self.object.is_null() {
                let create: unsafe extern "system" fn(
                    Device,
                    u32,
                    u32,
                    u32,
                    u32,
                    u32,
                    u32,
                    *mut *mut c_void,
                    *mut c_void,
                ) -> HResult = vfunc(device, 23);
                checked(
                    create(
                        device,
                        self.width as u32,
                        self.height as u32,
                        1,
                        0x200,
                        21,
                        0,
                        &mut self.object,
                        null_mut(),
                    ),
                    "IDirect3DDevice9::CreateTexture",
                )?;
                self.dirty = true;
            }
            if !self.dirty {
                return Ok(());
            }
            let lock: unsafe extern "system" fn(
                *mut c_void,
                u32,
                *mut LockedRect,
                *const Rect,
                u32,
            ) -> HResult = vfunc(self.object, 19);
            let unlock: unsafe extern "system" fn(*mut c_void, u32) -> HResult =
                vfunc(self.object, 20);
            let mut locked = LockedRect {
                pitch: 0,
                bits: null_mut(),
            };
            checked(
                lock(self.object, 0, &mut locked, null(), 0x2000),
                "IDirect3DTexture9::LockRect",
            )?;
            if locked.bits.is_null() || locked.pitch < (self.width * 4) as i32 {
                let _ = unlock(self.object, 0);
                return Err("D3D9 returned an invalid texture pitch".to_owned());
            }
            for row in 0..self.height {
                std::ptr::copy_nonoverlapping(
                    self.pixels.as_ptr().add(row * self.width * 4),
                    (locked.bits as *mut u8).add(row * locked.pitch as usize),
                    self.width * 4,
                );
            }
            checked(unlock(self.object, 0), "IDirect3DTexture9::UnlockRect")?;
            self.dirty = false;
            Ok(())
        }
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe { release(self.object) }
    }
}

struct StateBlock(*mut c_void);

impl StateBlock {
    unsafe fn capture(device: Device) -> Result<Self, String> {
        unsafe {
            let create: unsafe extern "system" fn(Device, u32, *mut *mut c_void) -> HResult =
                vfunc(device, 59);
            let mut pointer = null_mut();
            checked(create(device, 1, &mut pointer), "D3D9 state capture")?;
            let state = Self(pointer);
            let capture: unsafe extern "system" fn(*mut c_void) -> HResult = vfunc(pointer, 4);
            checked(capture(pointer), "IDirect3DStateBlock9::Capture")?;
            Ok(state)
        }
    }
}

impl Drop for StateBlock {
    fn drop(&mut self) {
        unsafe {
            let apply: unsafe extern "system" fn(*mut c_void) -> HResult = vfunc(self.0, 5);
            let _ = apply(self.0);
            release(self.0);
        }
    }
}

pub struct Painter {
    device: Device,
    textures: HashMap<TextureId, Texture>,
    vertices: Vec<Vertex>,
}

unsafe impl Send for Painter {}

impl Painter {
    pub unsafe fn new(device: Device) -> Result<Self, String> {
        unsafe {
            if device.is_null() {
                return Err("D3D9 device is null".to_owned());
            }
            let add_ref: unsafe extern "system" fn(Device) -> u32 = vfunc(device, 1);
            add_ref(device);
            Ok(Self {
                device,
                textures: HashMap::new(),
                vertices: Vec::new(),
            })
        }
    }

    pub fn matches(&self, device: Device) -> bool {
        self.device == device
    }

    pub unsafe fn set_device(&mut self, device: Device) -> Result<(), String> {
        unsafe {
            if device.is_null() {
                return Err("D3D9 replacement device is null".to_owned());
            }
            if self.matches(device) {
                return Ok(());
            }
            let add_ref: unsafe extern "system" fn(Device) -> u32 = vfunc(device, 1);
            add_ref(device);
            self.invalidate();
            release(self.device);
            self.device = device;
            Ok(())
        }
    }

    pub fn invalidate(&mut self) {
        for texture in self.textures.values_mut() {
            unsafe { texture.invalidate() }
        }
    }

    fn update_texture(
        &mut self,
        id: TextureId,
        image: &egui::epaint::ImageDelta,
    ) -> Result<(), String> {
        let [width, height] = image.image.size();
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err("Invalid egui texture dimensions".to_owned());
        }
        let length = width
            .checked_mul(height)
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| "Texture size overflow".to_owned())?;
        let mut pixels = Vec::with_capacity(length);
        match &image.image {
            ImageData::Color(color) => {
                if color.pixels.len() != width * height {
                    return Err("Invalid egui color texture length".to_owned());
                }
                for pixel in &color.pixels {
                    let [r, g, b, a] = pixel.to_array();
                    pixels.extend_from_slice(&[b, g, r, a]);
                }
            }
            ImageData::Font(font) => {
                for pixel in font.srgba_pixels(None) {
                    let [r, g, b, a] = pixel.to_array();
                    pixels.extend_from_slice(&[b, g, r, a]);
                }
                if pixels.len() != length {
                    return Err("Invalid egui font texture length".to_owned());
                }
            }
        }
        if let Some([left, top]) = image.pos {
            let texture = self
                .textures
                .get_mut(&id)
                .ok_or_else(|| "Partial update of an unknown egui texture".to_owned())?;
            if left
                .checked_add(width)
                .is_none_or(|end| end > texture.width)
                || top
                    .checked_add(height)
                    .is_none_or(|end| end > texture.height)
            {
                return Err("Egui texture update is outside the atlas".to_owned());
            }
            for row in 0..height {
                let begin = ((top + row) * texture.width + left) * 4;
                texture.pixels[begin..begin + width * 4]
                    .copy_from_slice(&pixels[row * width * 4..(row + 1) * width * 4]);
            }
            texture.options = image.options;
            texture.dirty = true;
        } else {
            self.textures.insert(
                id,
                Texture {
                    object: null_mut(),
                    width,
                    height,
                    pixels,
                    options: image.options,
                    dirty: true,
                },
            );
        }
        Ok(())
    }

    unsafe fn configure(&self, width: u32, height: u32) -> Result<(), String> {
        unsafe {
            let set_viewport: unsafe extern "system" fn(Device, *const Viewport) -> HResult =
                vfunc(self.device, 47);
            checked(
                set_viewport(
                    self.device,
                    &Viewport {
                        x: 0,
                        y: 0,
                        width,
                        height,
                        min_z: 0.0,
                        max_z: 1.0,
                    },
                ),
                "D3D9 viewport",
            )?;
            let set_shader: unsafe extern "system" fn(Device, *mut c_void) -> HResult =
                vfunc(self.device, 92);
            let set_pixel_shader: unsafe extern "system" fn(Device, *mut c_void) -> HResult =
                vfunc(self.device, 107);
            checked(set_shader(self.device, null_mut()), "D3D9 vertex shader")?;
            checked(
                set_pixel_shader(self.device, null_mut()),
                "D3D9 pixel shader",
            )?;
            let set_fvf: unsafe extern "system" fn(Device, u32) -> HResult = vfunc(self.device, 89);
            checked(set_fvf(self.device, 0x144), "D3D9 vertex format")?;
            let set_patch: unsafe extern "system" fn(Device, f32) -> HResult =
                vfunc(self.device, 79);
            checked(set_patch(self.device, 0.0), "D3D9 patch mode")?;
            let render_state: unsafe extern "system" fn(Device, u32, u32) -> HResult =
                vfunc(self.device, 57);
            for (state, value) in [
                (7, 0),
                (8, 3),
                (9, 2),
                (14, 0),
                (15, 0),
                (19, 2),
                (20, 6),
                (22, 1),
                (27, 1),
                (28, 0),
                (29, 0),
                (34, 0),
                (52, 0),
                (128, 0),
                (136, 1),
                (137, 0),
                (141, 1),
                (142, 1),
                (143, 1),
                (145, 1),
                (146, 2),
                (147, 1),
                (152, 0),
                (161, 1),
                (162, u32::MAX),
                (168, 15),
                (171, 1),
                (174, 1),
                (175, 0),
                (176, 0),
                (194, 0),
                (206, 1),
                (207, 2),
                (208, 6),
                (209, 1),
            ] {
                checked(render_state(self.device, state, value), "D3D9 render state")?;
            }
            let texture_stage: unsafe extern "system" fn(Device, u32, u32, u32) -> HResult =
                vfunc(self.device, 67);
            for (stage, state, value) in [
                (0, 1, 4),
                (0, 2, 2),
                (0, 3, 0),
                (0, 4, 4),
                (0, 5, 2),
                (0, 6, 0),
                (0, 11, 0),
                (0, 24, 0),
                (0, 28, 1),
                (1, 1, 1),
                (1, 4, 1),
            ] {
                checked(
                    texture_stage(self.device, stage, state, value),
                    "D3D9 texture stage",
                )?;
            }
            Ok(())
        }
    }

    pub unsafe fn paint(
        &mut self,
        primitives: &[ClippedPrimitive],
        textures: &TexturesDelta,
        pixels_per_point: f32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        unsafe {
            for (id, image) in &textures.set {
                self.update_texture(*id, image)?;
            }
            let result = self.paint_meshes(primitives, pixels_per_point, width, height);
            for id in &textures.free {
                self.textures.remove(id);
            }
            result
        }
    }

    unsafe fn paint_meshes(
        &mut self,
        primitives: &[ClippedPrimitive],
        pixels_per_point: f32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        unsafe {
            if width == 0 || height == 0 || !pixels_per_point.is_finite() || pixels_per_point <= 0.0
            {
                return Ok(());
            }
            for primitive in primitives {
                if let Primitive::Mesh(mesh) = &primitive.primitive {
                    if let Some(texture) = self.textures.get_mut(&mesh.texture_id) {
                        texture.upload(self.device)?;
                    }
                }
            }
            if primitives.is_empty() {
                return Ok(());
            }
            let _state = StateBlock::capture(self.device)?;
            self.configure(width, height)?;
            let set_scissor: unsafe extern "system" fn(Device, *const Rect) -> HResult =
                vfunc(self.device, 75);
            let set_texture: unsafe extern "system" fn(Device, u32, *mut c_void) -> HResult =
                vfunc(self.device, 65);
            let set_sampler: unsafe extern "system" fn(Device, u32, u32, u32) -> HResult =
                vfunc(self.device, 69);
            let draw: unsafe extern "system" fn(
                Device,
                u32,
                u32,
                u32,
                u32,
                *const c_void,
                u32,
                *const c_void,
                u32,
            ) -> HResult = vfunc(self.device, 84);
            for clipped in primitives {
                let Primitive::Mesh(mesh) = &clipped.primitive else {
                    continue;
                };
                if mesh.vertices.is_empty() || mesh.indices.is_empty() {
                    continue;
                }
                if !mesh.is_valid() {
                    return Err("Invalid egui mesh indices".to_owned());
                }
                let Some(texture) = self.textures.get(&mesh.texture_id) else {
                    return Err(format!("Missing egui texture {:?}", mesh.texture_id));
                };
                let clip = clipped.clip_rect;
                let rect = Rect {
                    left: (clip.min.x * pixels_per_point)
                        .round()
                        .clamp(0.0, width as f32) as i32,
                    top: (clip.min.y * pixels_per_point)
                        .round()
                        .clamp(0.0, height as f32) as i32,
                    right: (clip.max.x * pixels_per_point)
                        .round()
                        .clamp(0.0, width as f32) as i32,
                    bottom: (clip.max.y * pixels_per_point)
                        .round()
                        .clamp(0.0, height as f32) as i32,
                };
                if rect.right <= rect.left || rect.bottom <= rect.top {
                    continue;
                }
                checked(set_scissor(self.device, &rect), "D3D9 scissor")?;
                checked(set_texture(self.device, 0, texture.object), "D3D9 texture")?;
                let filter = |value| match value {
                    TextureFilter::Nearest => 1,
                    TextureFilter::Linear => 2,
                };
                let address = match texture.options.wrap_mode {
                    TextureWrapMode::ClampToEdge => 3,
                    TextureWrapMode::Repeat => 1,
                    TextureWrapMode::MirroredRepeat => 2,
                };
                for (state, value) in [
                    (1, address),
                    (2, address),
                    (3, address),
                    (5, filter(texture.options.magnification)),
                    (6, filter(texture.options.minification)),
                    (7, 0),
                    (11, 0),
                ] {
                    checked(set_sampler(self.device, 0, state, value), "D3D9 sampler")?;
                }
                self.vertices.clear();
                self.vertices.reserve(mesh.vertices.len());
                for vertex in &mesh.vertices {
                    let [r, g, b, a] = vertex.color.to_array();
                    self.vertices.push(Vertex {
                        x: vertex.pos.x * pixels_per_point - 0.5,
                        y: vertex.pos.y * pixels_per_point - 0.5,
                        z: 0.0,
                        rhw: 1.0,
                        color: u32::from_le_bytes([b, g, r, a]),
                        u: vertex.uv.x,
                        v: vertex.uv.y,
                    });
                }
                checked(
                    draw(
                        self.device,
                        4,
                        0,
                        self.vertices.len() as u32,
                        (mesh.indices.len() / 3) as u32,
                        mesh.indices.as_ptr() as *const c_void,
                        102,
                        self.vertices.as_ptr() as *const c_void,
                        std::mem::size_of::<Vertex>() as u32,
                    ),
                    "IDirect3DDevice9::DrawIndexedPrimitiveUP",
                )?;
            }
            Ok(())
        }
    }
}

impl Drop for Painter {
    fn drop(&mut self) {
        self.textures.clear();
        unsafe { release(self.device) }
    }
}
