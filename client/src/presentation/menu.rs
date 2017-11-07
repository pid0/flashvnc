use ::EncodingQuality;

const KEY_F1 : u32 = 0xffbe;
const KEY_F2 : u32 = 0xffbf;
const KEY_F3 : u32 = 0xffc0;
const KEY_F4 : u32 = 0xffc1;
const KEY_F5 : u32 = 0xffc2;
const KEY_F6 : u32 = 0xffc3;
const KEY_F8 : u32 = 0xffc5;
const KEY_F11 : u32 = 0xffc8;

pub struct Menu<H : MenuActionHandler> {
    handler : H,
    f8_pressed : bool,
    relative_mouse_mode : bool,
    fullscreen : bool
}
impl<H : MenuActionHandler> Menu<H> {
    pub fn new(handler : H) -> Self {
        Self {
            handler: handler,
            f8_pressed: false,
            relative_mouse_mode: false,
            fullscreen: false
        }
    }
    pub fn intercept_key_press(&mut self, keysym : u32) -> bool {
        let f8_pressed_now = keysym == KEY_F8;
        let f8_was_pressed = self.f8_pressed;
        if f8_was_pressed {
            match keysym {
                KEY_F1 => {
                    self.handler.set_encoding_quality(
                        EncodingQuality::LossyHigh);
                },
                KEY_F2 => {
                    self.handler.set_encoding_quality(
                        EncodingQuality::LossyMedium);
                },
                KEY_F3 => {
                    self.handler.set_encoding_quality(
                        EncodingQuality::LossyMediumInterframeComparison);
                },
                KEY_F4 => {
                    self.handler.set_encoding_quality(
                        EncodingQuality::LossyLow);
                },
                KEY_F5 => {
                    self.handler.set_encoding_quality(
                        EncodingQuality::Lossless);
                },
                KEY_F6 => {
                    self.relative_mouse_mode = !self.relative_mouse_mode;
                    if self.relative_mouse_mode {
                        self.handler.start_relative_mouse_mode();
                    } else {
                        self.handler.stop_relative_mouse_mode();
                    }
                },
                KEY_F11 => {
                    self.fullscreen = !self.fullscreen;
                    if self.fullscreen {
                        self.handler.set_fullscreen();
                    } else {
                        self.handler.unset_fullscreen();
                    }
                },
                _ => { }
            }
            self.f8_pressed = false;
        } else if f8_pressed_now {
            self.f8_pressed = true;
        }

        if f8_was_pressed || f8_pressed_now {
            return true;
        }

        false
    }

    pub fn visible(&self) -> bool {
        self.f8_pressed
    }
    pub fn relative_mouse_mode(&self) -> bool {
        self.relative_mouse_mode
    }

    pub fn draw<D : DrawingContext>(&self, d : &mut D,
                                    width : f64, _height : f64) {
        let item_width = width * 0.9;
        let item_height = 35.0;
        let item_spacing = 40.0;

        for (i, &(text, on)) in [
            ("F1: Encoding: Lossy, high quality", None),
            ("F2: Encoding: Lossy, medium quality", None),
            ("F3: Encoding: Lossy, medium, with interframe comparison", None),
            ("F4: Encoding: Lossy, low quality", None),
            ("F5: Encoding: Lossless", None),
            ("F6: Relative mouse mode", Some(self.relative_mouse_mode)),
            ("F11: Fullscreen", Some(self.fullscreen))
        ].iter().enumerate() {
            let y = (i as f64) * item_spacing;
            d.fill_background_rect(0.0, y, item_width, item_height);

            let text_x = 5.0;
            let text_y = y + item_spacing / 2.0;
            if let Some(on) = on {
                d.draw_text(text_x, text_y, 
                            &format!("[{}] {}",
                                     if on {
                                         "x"
                                     } else {
                                         " "
                                     }, text));
            } else {
                d.draw_text(text_x, text_y, text);
            }
        }
    }
}

pub trait MenuActionHandler {
    fn set_encoding_quality(&mut self, quality : EncodingQuality);
    fn set_fullscreen(&mut self);
    fn unset_fullscreen(&mut self);
    fn start_relative_mouse_mode(&mut self);
    fn stop_relative_mouse_mode(&mut self);
}

pub trait DrawingContext {
    fn fill_background_rect(&mut self, x : f64, y : f64, w : f64, h : f64);
    fn draw_text(&mut self, x : f64, y : f64, text : &str);
}
