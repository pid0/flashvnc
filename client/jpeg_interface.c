#include <stdio.h>
#include <assert.h>
#include <stdlib.h>
#include <jpeglib.h>
#include <setjmp.h>
#include <unistd.h>

#define BYTES_PER_PIXEL 3
#define OK 0
#define ERROR 1

struct ErrorManager {
    struct jpeg_error_mgr base;
    jmp_buf longjump_buf;
};
static void error_exit(j_common_ptr state) {
    struct ErrorManager* error_mgr = (struct ErrorManager*)state->err;
    longjmp(error_mgr->longjump_buf, 1);
}

typedef void* JpegState;
struct DecoderState {
    struct jpeg_decompress_struct base;
    struct ErrorManager error_manager;
    unsigned char* scanline;
    size_t in_stride;
};

JpegState jpgint_dec_new() {
    struct DecoderState* ret = malloc(sizeof(struct DecoderState));
    assert(ret != NULL);

    ret->base.err = jpeg_std_error(&ret->error_manager.base);
    ret->error_manager.base.error_exit = error_exit;

    if(setjmp(ret->error_manager.longjump_buf) != 0) {
        assert(0);
    }

    jpeg_create_decompress(&ret->base);
    ret->scanline = NULL;
    return (JpegState)ret;
}

int jpgint_dec_start(JpegState opaque_state, unsigned char const* src,
        size_t len) {
    struct DecoderState* state = (struct DecoderState*)opaque_state;
    struct jpeg_decompress_struct* jpeg_state = &state->base;

    if(setjmp(state->error_manager.longjump_buf) != 0) {
        return ERROR;
    }

//    jpeg_stdio_src(&state, stdin);
    jpeg_mem_src(jpeg_state, src, len);
    jpeg_read_header(jpeg_state, TRUE);
    //TODO set output colorspace (see "Special color spaces", JCS_EXT_RGB)
    jpeg_start_decompress(jpeg_state); //now: state.{output_width,output_height,output_components}

    state->in_stride = jpeg_state->output_width * jpeg_state->output_components;
    unsigned char* scanline_buffer = malloc(state->in_stride);
    assert(scanline_buffer != NULL);
    state->scanline = scanline_buffer;

    return OK;
}

int jpgint_dec_next_line(JpegState opaque_state, unsigned char* dest) {
    struct DecoderState* state = (struct DecoderState*)opaque_state;
    struct jpeg_decompress_struct* jpeg_state = &state->base;

    if(setjmp(state->error_manager.longjump_buf) != 0) {
        return ERROR;
    }

    if(BYTES_PER_PIXEL == jpeg_state->output_components) {
        jpeg_read_scanlines(jpeg_state, &dest, 1);
    } else {
        jpeg_read_scanlines(jpeg_state, &state->scanline, 1);
        assert(jpeg_state->output_components == 1);
        for(size_t i = 0; i < jpeg_state->output_width; ++i) {
            dest[i * BYTES_PER_PIXEL + 0] = state->scanline[i];
            dest[i * BYTES_PER_PIXEL + 1] = state->scanline[i];
            dest[i * BYTES_PER_PIXEL + 2] = state->scanline[i];
        }
    }

    return OK;
}

static void finish_decompress_void(struct jpeg_decompress_struct* state) {
    jpeg_finish_decompress(state);
}
static void end_decompression(
        JpegState opaque_state,
        void (*end)(struct jpeg_decompress_struct*)) {
    struct DecoderState* state = (struct DecoderState*)opaque_state;
    struct jpeg_decompress_struct* jpeg_state = &state->base;

    if(setjmp(state->error_manager.longjump_buf) != 0) {
        assert(0);
    }

    end(jpeg_state);
    free(state->scanline);
    state->scanline = NULL;
}
void jpgint_dec_end(JpegState opaque_state) {
    end_decompression(opaque_state, finish_decompress_void);
}
void jpgint_dec_abort(JpegState opaque_state) {
    end_decompression(opaque_state, jpeg_abort_decompress);
}
void jpgint_dec_destroy(JpegState opaque_state) {
    struct DecoderState* state = (struct DecoderState*)opaque_state;
    struct jpeg_decompress_struct* jpeg_state = &state->base;

    if(setjmp(state->error_manager.longjump_buf) != 0) {
        assert(0);
    }

    jpeg_destroy_decompress(jpeg_state);
    free(state->scanline);
    free(state);
}

size_t jpgint_dec_get_width(JpegState opaque_state) {
    struct DecoderState* state = (struct DecoderState*)opaque_state;
    struct jpeg_decompress_struct* jpeg_state = &state->base;
    return jpeg_state->output_width;
}
size_t jpgint_dec_get_height(JpegState opaque_state) {
    struct DecoderState* state = (struct DecoderState*)opaque_state;
    struct jpeg_decompress_struct* jpeg_state = &state->base;
    return jpeg_state->output_height;
}

void jpgint_get_error(JpegState opaque_state, char* dest) {
    struct jpeg_common_struct* state = (struct jpeg_common_struct*)opaque_state;
    (*state->err->format_message)((struct jpeg_common_struct*)state, dest);
}
