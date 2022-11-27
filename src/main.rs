#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused)]

mod fbc {include!("./bindings/nv_fbc.rs");}
mod enc {include!("./bindings/nv_enc.rs");}
mod cuda {include!("./bindings/cuda.rs");}


extern crate libloading;
use libloading::Library;

use crate::enc::{NV_ENC_PRESET_LOW_LATENCY_DEFAULT_GUID, NV_ENC_PRESET_LOW_LATENCY_HP_GUID, NV_ENC_PRESET_DEFAULT_GUID};

fn main() {
unsafe {
    /*
     * Dynamically load the NVidia libraries.
     */
    let nv_fbc = fbc::NvFBC::new("/lib/x86_64-linux-gnu/libnvidia-fbc.so.1").unwrap();   // TODO: Add proper library discovery.
    let nv_enc = enc::NvEnc::new("/lib/x86_64-linux-gnu/libnvidia-encode.so.1").unwrap();//
    let nv_cuda = cuda::cuda::new("/lib/x86_64-linux-gnu/libcuda.so.1").unwrap();

    /*
     * Initialize CUDA. 
     */
    let mut cu_ctx = std::mem::zeroed::<cuda::CUcontext>();
    let mut cu_dev = std::mem::zeroed::<cuda::CUdevice>();

    let mut cu_res = nv_cuda.cuInit(0);
    if cu_res != cuda::cudaError_enum::CUDA_SUCCESS {
        panic!("Unable to initialize CUDA context. Result: {}", cu_res as u32);
    }

    cu_res = nv_cuda.cuDeviceGet(&mut cu_dev, 0);
    if cu_res != cuda::cudaError_enum::CUDA_SUCCESS {
        panic!("Unable to get CUDA device. Result: {}", cu_res as u32);
    }

    cu_res = nv_cuda.cuCtxCreate_v2(&mut cu_ctx, cuda::CUctx_flags::CU_CTX_SCHED_AUTO as u32, cu_dev);
    if cu_res != cuda::cudaError_enum::CUDA_SUCCESS {
        panic!("Unable to create CUDA context. Result: {}", cu_res as u32);
    }

    /*
     * Create an NvFBC instance.
     *
     * API function pointers are accessible through cap_fn.
     */
    let mut cap_fn = std::mem::zeroed::<fbc::NVFBC_API_FUNCTION_LIST>();
    cap_fn.dwVersion = nvfbc_version();
    
    let mut fbc_status = nv_fbc.NvFBCCreateInstance(&mut cap_fn);
    if fbc_status != fbc::NVFBCSTATUS::NVFBC_SUCCESS {
        panic!("Failed to create NvFBC instance. Status = {}, exiting", fbc_status as u32);
    }

    /*
     * Create an NvEnc instance.
     *
     * API function pointers are accesible through enc_fn.
     */
    let mut enc_fn = std::mem::zeroed::<enc::NV_ENCODE_API_FUNCTION_LIST>();
    enc_fn.version = nvenc_struct_version(2);

    let mut enc_status = nv_enc.NvEncodeAPICreateInstance(&mut enc_fn);
    if enc_status != enc::NVENCSTATUS::NV_ENC_SUCCESS {
        panic!("Failed to create NvEnc instance. Status = {}, exiting", enc_status as u32);
    }

    /*
     * Create a session handle that is used to identify the client.
     */
    let mut fbc_create_handle_params = std::mem::zeroed::<fbc::NVFBC_CREATE_HANDLE_PARAMS>();
    fbc_create_handle_params.dwVersion = nvfbc_struct_version::<fbc::NVFBC_CREATE_HANDLE_PARAMS>(2);

    let mut fbc_handle = std::mem::zeroed::<fbc::NVFBC_SESSION_HANDLE>();

    fbc_status = cap_fn.nvFBCCreateHandle.unwrap()(&mut fbc_handle, &mut fbc_create_handle_params);
    if fbc_status == fbc::NVFBCSTATUS::NVFBC_ERR_UNSUPPORTED {
        println!("Your hardware doesn't support NvFBC or is unpatched");
        println!("Ensure you have a supported GPU and if you have a consumer level GPU, apply this patch:");
        println!("      https://github.com/keylase/nvidia-patch");
        println!("(please make sure to apply patch-fbc.sh)");
    }

    if fbc_status != fbc::NVFBCSTATUS::NVFBC_SUCCESS {
        panic!("Failed to create NvFBC handle. Status = {}, exiting", fbc_status as u32);
    }

    /*
     * Get information about the state of the display driver.
     *
     * This call is optional but helps the application decide what it should
     * do.
     */
    let mut fbc_status_params = std::mem::zeroed::<fbc::NVFBC_GET_STATUS_PARAMS>();
    fbc_status_params.dwVersion = nvfbc_struct_version::<fbc::NVFBC_GET_STATUS_PARAMS>(2);

    fbc_status = cap_fn.nvFBCGetStatus.unwrap()(fbc_handle, &mut fbc_status_params);
    if fbc_status != fbc::NVFBCSTATUS::NVFBC_SUCCESS {
        let error = std::ffi::CStr::from_ptr(cap_fn.nvFBCGetLastErrorStr.unwrap()(fbc_handle)).to_str().unwrap();
        panic!("{}", error);
    }

    if fbc_status_params.bCanCreateNow == fbc::NVFBC_BOOL::NVFBC_FALSE {
        panic!("It is not possible to create a capture session on this system");
    }

    /*
     * Create a capture session.
     */
    let frame_size = fbc::NVFBC_SIZE {
        w: 3440,
        h: 1440,
    };

    let mut fbc_create_capture_params = std::mem::zeroed::<fbc::NVFBC_CREATE_CAPTURE_SESSION_PARAMS>();
    fbc_create_capture_params.dwVersion = nvfbc_struct_version::<fbc::NVFBC_CREATE_CAPTURE_SESSION_PARAMS>(6);
    fbc_create_capture_params.eCaptureType = fbc::NVFBC_CAPTURE_TYPE::NVFBC_CAPTURE_SHARED_CUDA;
    fbc_create_capture_params.bWithCursor = fbc::NVFBC_BOOL::NVFBC_TRUE;
    fbc_create_capture_params.frameSize = frame_size;
    fbc_create_capture_params.eTrackingType = fbc::NVFBC_TRACKING_TYPE::NVFBC_TRACKING_DEFAULT;

    fbc_status = cap_fn.nvFBCCreateCaptureSession.unwrap()(fbc_handle, &mut fbc_create_capture_params);
    if fbc_status != fbc::NVFBCSTATUS::NVFBC_SUCCESS {
        let error = std::ffi::CStr::from_ptr(cap_fn.nvFBCGetLastErrorStr.unwrap()(fbc_handle)).to_str().unwrap();
        panic!("{}", error);
    }

    /*
     * Set up the capture session.
     */
    let mut fbc_setup_params = std::mem::zeroed::<fbc::NVFBC_TOCUDA_SETUP_PARAMS>();
    fbc_setup_params.dwVersion = nvfbc_struct_version::<fbc::NVFBC_TOCUDA_SETUP_PARAMS>(1);
    fbc_setup_params.eBufferFormat = fbc::NVFBC_BUFFER_FORMAT::NVFBC_BUFFER_FORMAT_NV12;

    fbc_status = cap_fn.nvFBCToCudaSetUp.unwrap()(fbc_handle, &mut fbc_setup_params);
    if fbc_status != fbc::NVFBCSTATUS::NVFBC_SUCCESS {
        let error = std::ffi::CStr::from_ptr(cap_fn.nvFBCGetLastErrorStr.unwrap()(fbc_handle)).to_str().unwrap();
        panic!("{}", error);
    }

    /*
     * Create an encoder session.
     */
    let mut enc_session_params = std::mem::zeroed::<enc::NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS>();
    enc_session_params.version = nvenc_struct_version(1);
    enc_session_params.apiVersion = enc::NVENCAPI_VERSION;
    enc_session_params.deviceType = enc::NV_ENC_DEVICE_TYPE::NV_ENC_DEVICE_TYPE_CUDA;
    enc_session_params.device = cu_ctx as *mut std::ffi::c_void;

    let mut encoder = std::mem::zeroed::<*mut std::ffi::c_void>();

    enc_status = enc_fn.nvEncOpenEncodeSessionEx.unwrap()(&mut enc_session_params, &mut encoder);
    if enc_status != enc::NVENCSTATUS::NV_ENC_SUCCESS {
        panic!("Failed to open an encoder session. Status = {}", enc_status as u32);
    }

    /*
     * Validate the codec.
     */

    const codec_h264: enc::_GUID = enc::_GUID {
        Data1: 0x6bc82762,
        Data2: 0x4e63,
        Data3: 0x4d7b,
        Data4: [0x94, 0x25, 0xbd, 0xa9, 0x97, 0x5f, 0x76, 0x3]
    };

    const preset_low_latency: enc::_GUID = enc::_GUID {
        Data1: 0x49df21c5,
        Data2: 0x6dfa,
        Data3: 0x4feb,
        Data4: [0x97, 0x87, 0x6a, 0xcc, 0x9e, 0xff, 0xb7, 0x26]
    };

    // let mut enc_guid_count = 0;
    // enc_status = enc_fn.nvEncGetEncodeGUIDCount.unwrap()(encoder, &mut enc_guid_count);
    // if enc_status != enc::NVENCSTATUS::NV_ENC_SUCCESS {
    //     panic!("Failed to query number of supported codecs. Status = {}", enc_status as u32);
    // }
    
    // let mut enc_guid_array = Vec::<enc::GUID>::with_capacity(enc_guid_count as usize);
    // let mut enc_nguids = 0;

    // enc_status = enc_fn.nvEncGetEncodeGUIDs.unwrap()(encoder, enc_guid_array.as_mut_ptr(), enc_guid_count, &mut enc_nguids);
    // if enc_status != enc::NVENCSTATUS::NV_ENC_SUCCESS {
    //     panic!("Failed to query number of supported codecs. Status = {}", enc_status as u32);
    // }
    
    // let mut codec_found = false;

    // for i in 0..enc_nguids {
    //     if codec_h264.Data1 == enc_guid_array[i as usize].Data1 &&
    //        codec_h264.Data2 == enc_guid_array[i as usize].Data2 &&
    //        codec_h264.Data3 == enc_guid_array[i as usize].Data3 &&
    //        codec_h264.Data4 == enc_guid_array[i as usize].Data4 
    //     {
    //         codec_found = true;
    //         break;
    //     }
    // }

    // if !codec_found {
    //     panic!("Could not enumerate the H264 codec");
    // }
    let mut enc_preset_config = enc::NV_ENC_PRESET_CONFIG {
        version: nvenc_struct_version(4) | (1<<31),
        presetCfg: enc::NV_ENC_CONFIG {
            version: nvenc_struct_version(6) | (1<<31),
            profileGUID: todo!(),
            gopLength: todo!(),
            frameIntervalP: todo!(),
            monoChromeEncoding: 0,
            frameFieldMode: todo!(),
            mvPrecision: todo!(),
            rcParams: enc::NV_ENC_RC_PARAMS {
                version: todo!(),
                rateControlMode: todo!(),
                constQP: todo!(),
                averageBitRate: todo!(),
                maxBitRate: todo!(),
                vbvBufferSize: todo!(),
                vbvInitialDelay: todo!(),
                _bitfield_align_1: todo!(),
                _bitfield_1: todo!(),
                minQP: todo!(),
                maxQP: todo!(),
                initialRCQP: todo!(),
                temporallayerIdxMask: todo!(),
                temporalLayerQP: todo!(),
                targetQuality: todo!(),
                targetQualityLSB: todo!(),
                lookaheadDepth: todo!(),
                lowDelayKeyFrameScale: todo!(),
                yDcQPIndexOffset: todo!(),
                uDcQPIndexOffset: todo!(),
                vDcQPIndexOffset: todo!(),
                qpMapMode: todo!(),
                multiPass: todo!(),
                alphaLayerBitrateRatio: todo!(),
                cbQPIndexOffset: todo!(),
                crQPIndexOffset: todo!(),
                reserved2: 0,
                reserved: [0; 4],
            },
            encodeCodecConfig: enc::NV_ENC_CODEC_CONFIG {
                h264Config: enc::NV_ENC_CONFIG_H264 {
                    _bitfield_align_1: todo!(),
                    _bitfield_1: todo!(),
                    level: todo!(),
                    idrPeriod: todo!(),
                    separateColourPlaneFlag: todo!(),
                    disableDeblockingFilterIDC: todo!(),
                    numTemporalLayers: todo!(),
                    spsId: todo!(),
                    ppsId: todo!(),
                    adaptiveTransformMode: todo!(),
                    fmoMode: todo!(),
                    bdirectMode: todo!(),
                    entropyCodingMode: todo!(),
                    stereoMode: todo!(),
                    intraRefreshPeriod: todo!(),
                    intraRefreshCnt: todo!(),
                    maxNumRefFrames: todo!(),
                    sliceMode: todo!(),
                    sliceModeData: todo!(),
                    h264VUIParameters: todo!(),
                    ltrNumFrames: todo!(),
                    ltrTrustMode: todo!(),
                    chromaFormatIDC: todo!(),
                    maxTemporalLayers: todo!(),
                    useBFramesAsRef: todo!(),
                    numRefL0: todo!(),
                    numRefL1: todo!(),

                    reserved1: [0; 267],
                    reserved2: [std::ptr::null_mut(); 64],
                }
            },
            reserved: [0; 278],
            reserved2: [std::ptr::null_mut(); 64],
        },
        reserved1: [0; 255],
        reserved2: [std::ptr::null_mut(); 64],
    };
    
    enc_status = enc_fn.nvEncGetEncodePresetConfig.unwrap()(encoder, codec_h264, preset_low_latency, &mut enc_preset_config);
    if enc_status != enc::NVENCSTATUS::NV_ENC_SUCCESS {
        panic!("Failed to obtain encoder preset settings. Status = {}", enc_status as u32);
    }

    enc_preset_config.presetCfg.rcParams.averageBitRate = 5 * 1024 * 1024;
    enc_preset_config.presetCfg.rcParams.maxBitRate = 8 * 1024 * 1024;

    /*
     * We are now ready to start grabbing frames.
     */
    // let cu_dev_ptr: cuda::CUdeviceptr;
    // let &mut frame
    // loop {



    // }

    println!("I ran succesfully tf???");
}
}

fn nvfbc_version() -> u32 {
    fbc::NVFBC_VERSION_MINOR | (fbc::NVFBC_VERSION_MAJOR << 8)
}

fn nvfbc_struct_version<T>(ver: u32) -> u32 {
    std::mem::size_of::<T>() as u32 | ((ver) << 16) | (nvfbc_version() << 24)
}

fn nvenc_version() -> u32 {
    enc::NVENCAPI_MAJOR_VERSION | (enc::NVENCAPI_MINOR_VERSION << 24)
}

fn nvenc_struct_version(ver: u32) -> u32 {
    nvenc_version() | ((ver)<<16) | (0x7 << 28)
}
