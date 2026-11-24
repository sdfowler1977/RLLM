#[cfg(any(test, g1_regression))]
mod desktop_regression {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("loops-lanes").join("G1-fix4-desktop-proof").join("tmp").join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(windows)]
    fn link_directory(link: &Path, target: &Path) -> std::io::Result<()> {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return Ok(());
        }
        std::fs::create_dir(link)?;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        let canonical = std::fs::canonicalize(target)?;
        let mut display = canonical.to_string_lossy().replace('/', "\\");
        if let Some(rest) = display.strip_prefix(r"\\?\UNC\") {
            display = format!(r"\\{rest}");
        } else if let Some(rest) = display.strip_prefix(r"\\?\") {
            display = rest.to_string();
        }
        let print = display.trim_end_matches('\\').to_string();
        let substitute = format!(r"\??\{print}");
        let sub: Vec<u16> = substitute.encode_utf16().collect();
        let printed: Vec<u16> = print.encode_utf16().collect();
        let mut path = Vec::new();
        path.extend_from_slice(&sub);
        path.push(0);
        let print_offset = path.len() * 2;
        path.extend_from_slice(&printed);
        path.push(0);
        let path_bytes = path.len() * 2;
        let data_length = 8 + path_bytes;
        let mut buf = Vec::<u8>::new();
        buf.extend_from_slice(&0xA000_0003u32.to_le_bytes());
        buf.extend_from_slice(&(data_length as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&((sub.len() * 2) as u16).to_le_bytes());
        buf.extend_from_slice(&(print_offset as u16).to_le_bytes());
        buf.extend_from_slice(&((printed.len() * 2) as u16).to_le_bytes());
        for unit in path {
            buf.extend_from_slice(&unit.to_le_bytes());
        }
        let file = std::fs::OpenOptions::new().write(true).custom_flags(0x0200_0000 | 0x0020_0000).open(link)?;
        unsafe extern "system" {
            fn DeviceIoControl(handle: *mut std::ffi::c_void, code: u32, input: *const std::ffi::c_void, input_size: u32, output: *mut std::ffi::c_void, output_size: u32, returned: *mut u32, overlapped: *mut std::ffi::c_void) -> i32;
        }
        let mut returned = 0u32;
        let ok = unsafe { DeviceIoControl(file.as_raw_handle(), 0x0009_00A4, buf.as_ptr().cast(), buf.len() as u32, std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut()) };
        if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
    }

    #[cfg(unix)]
    fn link_directory(link: &Path, target: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg_attr(test, test)]
    fn revision_rejects_a_linked_parent() {
        let root = scratch("link-parent");
        let selected = root.join("selected");
        let outside = root.join("outside");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        link_directory(&selected.join("link"), &outside).unwrap();
        let err = match revision_target(&selected, "link/secret.txt") {
            Err(error) => error,
            Ok(_) => panic!("a linked parent was accepted"),
        };
        assert!(err.contains("link"), "{err}");
        assert_eq!(std::fs::read(outside.join("secret.txt")).unwrap(), b"secret");
    }

    #[cfg_attr(test, test)]
    fn revision_reads_binary_past_one_mebibyte() {
        let root = scratch("binary-revision");
        let selected = root.join("selected");
        std::fs::create_dir_all(selected.join("nested")).unwrap();
        let mut bytes = vec![0u8, 255];
        bytes.extend(std::iter::repeat(b'Q').take(1024 * 1024));
        std::fs::write(selected.join("nested").join("note.bin"), &bytes).unwrap();
        let target = revision_target(&selected, "nested/note.bin").unwrap();
        let mut file = open_revision_file(&target, 0).unwrap();
        let (got, partial, total) = read_revision_bytes(&mut file).unwrap();
        assert!(!partial);
        assert_eq!(total, bytes.len() as u64);
        assert_eq!(got, bytes);
    }

    #[cfg_attr(test, test)]
    fn limited_read_stops_before_a_declared_huge_file() {
        let root = scratch("huge-read");
        let path = root.join("big.bin");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(8 * 1024 * 1024).unwrap();
        drop(file);
        let err = read_path_limited(&path, 1024).unwrap_err();
        assert!(err.contains("exceeds"), "{err}");
    }

    #[cfg_attr(test, test)]
    fn failed_project_write_preserves_the_handle_and_not_a_replacement() {
        let root = scratch("partial-write");
        let dir = root.join("folder");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        let mut file = std::fs::OpenOptions::new().read(true).write(true).create(true).open(&path).unwrap();
        file.write_all(b"ours").unwrap();
        std::fs::rename(&path, dir.join("moved.txt")).unwrap();
        std::fs::write(&path, b"theirs").unwrap();
        let kept = keep_failed_project_output(&mut file, &path, &path_text(&root)).unwrap();
        assert_eq!(std::fs::read(&kept).unwrap(), b"ours");
        assert_eq!(std::fs::read(&path).unwrap(), b"theirs");
    }

    #[cfg_attr(test, test)]
    fn stream_deadline_does_not_restart_when_bytes_arrive() {
        struct Trickle { n: usize }
        impl std::io::Read for Trickle {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.n >= 20 { return Ok(0); }
                std::thread::sleep(std::time::Duration::from_millis(20));
                self.n += 1;
                buf[0] = b'a';
                Ok(1)
            }
        }
        let mut stream = Trickle { n: 0 };
        let err = collect_until(&mut stream, 1024 * 1024, std::time::Instant::now() + std::time::Duration::from_millis(50)).unwrap_err();
        assert!(err.contains("two minutes"), "{err}");
        assert!(stream.n < 8, "deadline restarted, accepted {} bytes", stream.n);
    }

    #[cfg_attr(test, test)]
    fn empty_capture_is_not_a_fatal_read() {
        struct Empty { hits: u8 }
        impl std::io::Read for Empty {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.hits += 1;
                if self.hits == 1 {
                    return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "empty"));
                }
                buf[0] = 7;
                Ok(1)
            }
        }
        let mut empty = Empty { hits: 0 };
        let mut buf = [0u8; 4];
        let err = read_capture(&mut empty, &mut buf, std::time::Instant::now()).unwrap_err();
        assert!(err.contains("deadline"), "{err}");
        assert!(!err.contains("Microphone stream"));
        let mut once = Empty { hits: 0 };
        let n = read_capture(&mut once, &mut buf, std::time::Instant::now() + std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], 7);
    }

    #[cfg_attr(test, test)]
    fn session_hello_matches_the_published_core_method() {
        let start = session_start();
        let hello_at = start.iter().position(|frame| frame.id == 0 && frame.opcode == CORE_HELLO).unwrap();
        let props_at = start.iter().position(|frame| frame.id == 1 && frame.opcode == CLIENT_UPDATE_PROPERTIES).unwrap();
        let sync_at = start.iter().position(|frame| frame.id == 0 && frame.opcode == CORE_SYNC).unwrap();
        assert!(hello_at < props_at && props_at < sync_at);
        let hello = &start[hello_at];
        assert_eq!(u32::from_le_bytes(hello.body[4..8].try_into().unwrap()), POD_STRUCT);
        assert!(hello.body.windows(4).any(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()) == CORE_VERSION));
        let props = &start[props_at];
        let key = b"application.name";
        assert!(props.body.windows(key.len()).any(|window| window == key));
        let sync = &start[sync_at];
        assert_eq!(u32::from_le_bytes(sync.body[4..8].try_into().unwrap()), POD_STRUCT);
        assert!(sync.body.len() > 16);
    }

    fn rate_object(value: u32) -> Vec<u8> {
        let mut prop = Vec::new();
        prop.extend_from_slice(&0x10002u32.to_le_bytes());
        prop.extend_from_slice(&0u32.to_le_bytes());
        prop.extend(pod_int(value));
        let mut body = Vec::new();
        body.extend_from_slice(&0x40003u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend(prop);
        let mut out = Vec::new();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&POD_OBJECT.to_le_bytes());
        out.extend(body);
        out
    }

    #[cfg_attr(test, test)]
    fn session_negotiation_returns_samples_only_after_a_capture_buffer() {
        let mut nego = SessionNegotiator::new();
        let idle = nego.push(&SoundFrame { id: 0, opcode: CORE_EVENT_PING, seq: 4, body: Vec::new(), mem: Vec::new() }).unwrap();
        assert!(idle.iter().all(|frame| frame.opcode != CORE_GET_REGISTRY));
        let replies = nego.push(&SoundFrame { id: 0, opcode: CORE_EVENT_DONE, seq: 0, body: Vec::new(), mem: Vec::new() }).unwrap();
        assert!(replies.iter().any(|frame| frame.opcode == CORE_GET_REGISTRY), "done did not ask for the registry");
        assert!(replies.iter().any(|frame| frame.opcode == CORE_SYNC), "registry was not followed by sync");
        assert_eq!(nego.registry, 2);
        assert!(!nego.ready());
        let global = {
            let mut body = pod_int(4);
            body.extend(pod_int(1));
            body.extend(pod_string("Audio/Source"));
            body.extend(pod_int(3));
            body
        };
        let created = nego.push(&SoundFrame { id: nego.registry, opcode: 0, seq: 0, body: global, mem: Vec::new() }).unwrap();
        assert!(created.iter().any(|frame| frame.opcode == CORE_CREATE_OBJECT));
        assert!(!nego.ready());
        nego.push(&SoundFrame { id: nego.capture, opcode: NODE_EVENT_SET_PARAM, seq: 0, body: rate_object(16000), mem: Vec::new() }).unwrap();
        nego.push(&SoundFrame { id: nego.capture, opcode: NODE_EVENT_PORT_BUFFERS, seq: 0, body: Vec::new(), mem: vec![vec![1, 2, 3, 4]] }).unwrap();
        assert!(nego.ready());
        assert_eq!(nego.samples, vec![1, 2, 3, 4]);
    }

    #[cfg_attr(test, test)]
    fn nested_format_rate_is_recognized_and_a_hostile_size_is_not() {
        assert!(session_has_rate(&rate_object(16000)));
        assert!(!session_has_rate(&rate_object(48000)));
        assert!(!session_has_rate(&pod_int(48000)));
        assert!(session_has_rate(&pod_int(16000)));
        let mut hostile = rate_object(16000);
        hostile[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!session_has_rate(&hostile));
        let mut nested = pod_struct(&[rate_object(16000)]);
        assert!(session_has_rate(&nested));
        nested[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!session_has_rate(&nested));
        let choice = {
            let child = pod_int(16000);
            let mut body = Vec::new();
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend(&child);
            let mut out = Vec::new();
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&POD_CHOICE.to_le_bytes());
            out.extend(body);
            out
        };
        assert!(session_has_rate(&choice));
        let mut deep = pod_int(16000);
        for _ in 0..32 {
            deep = pod_struct(&[deep]);
        }
        assert!(!session_has_rate(&deep));
    }

    #[cfg_attr(test, test)]
    fn stdin_line_stops_at_the_parse_limit() {
        let mut exact = std::io::Cursor::new(b"{\"a\":1}\n".to_vec());
        assert_eq!(next_line_bounded(&mut exact, 8).unwrap().unwrap(), "{\"a\":1}");
        let mut over = std::io::Cursor::new(vec![b'x'; 20]);
        let err = next_line_bounded(&mut over, 8).unwrap_err();
        assert!(err.contains("8 MiB"), "{err}");
        let mut split = std::io::Cursor::new(b"0123456789EXTRA".to_vec());
        let err = next_line_bounded(&mut split, 8).unwrap_err();
        assert!(err.contains("8 MiB"), "{err}");
    }

    #[cfg_attr(test, test)]
    fn swapped_directory_entry_cannot_escape_the_selected_folder() {
        let root = scratch("swap-entry");
        let selected = root.join("selected");
        let outside = root.join("outside");
        let gate = selected.join("gate");
        std::fs::create_dir_all(&gate).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(gate.join("secret.txt"), b"inside").unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        let bound = canonical_root(&selected).unwrap();
        let dir = open_root_dir(&bound).unwrap();
        std::fs::remove_dir_all(&gate).unwrap();
        link_directory(&gate, &outside).unwrap();
        let opened = walk_once(&bound, Path::new("gate/secret.txt"), 0, &LinkEnd::Follow);
        match opened {
            Ok(Walked::Opened(mut file)) => {
                let mut buf = Vec::new();
                use std::io::Read;
                file.read_to_end(&mut buf).ok();
                assert_ne!(buf, b"secret");
            }
            Ok(Walked::Follow(next)) => {
                assert!(path_contained(&bound, &bound.join(&next)), "{next:?}");
                let err = confined_open(&selected, &selected.join("gate").join("secret.txt"), 0, LinkEnd::Follow).unwrap_err();
                assert!(err.to_ascii_lowercase().contains("selected folder") || err.to_ascii_lowercase().contains("link"), "{err}");
            }
            Err(error) => {
                assert!(error.to_ascii_lowercase().contains("selected folder") || error.to_ascii_lowercase().contains("link"), "{error}");
            }
        }
        drop(dir);
        let err = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("gate/secret.txt"))])),
        ]), &path_text(&selected)).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("selected folder") || err.to_ascii_lowercase().contains("link"), "{err}");
        assert_eq!(std::fs::read(outside.join("secret.txt")).unwrap(), b"secret");
    }

    #[cfg_attr(test, test)]
    fn in_folder_link_still_reads_the_target_file() {
        let root = scratch("inside-link");
        let selected = root.join("selected");
        let nested = selected.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("note.txt"), b"note").unwrap();
        link_directory(&selected.join("link"), &nested).unwrap();
        let text = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("link/note.txt"))])),
        ]), &path_text(&selected)).unwrap();
        assert_eq!(text, "note");
    }

    #[cfg(windows)]
    #[cfg_attr(test, test)]
    fn wave_header_matches_the_published_record() {
        assert_eq!(std::mem::size_of::<WaveHdr>(), std::mem::size_of::<usize>() * 4 + 16);
        assert_eq!(std::mem::offset_of!(WaveHdr, next), std::mem::size_of::<usize>() * 2 + 16);
    }

    #[cfg(windows)]
    #[cfg_attr(test, test)]
    fn failed_microphone_open_closes_the_event_handle() {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateEventW(attrs: *mut std::ffi::c_void, manual: i32, initial: i32, name: *const u16) -> isize;
            fn GetHandleInformation(handle: isize, flags: *mut u32) -> i32;
        }
        for status in [1u32, 7, 32] {
            let event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
            assert_ne!(event, 0, "the microphone event could not be created");
            let mut flags = 0u32;
            assert_ne!(unsafe { GetHandleInformation(event, &mut flags) }, 0, "event was not live before the failed open");
            let error = reject_failed_wave_open(event, status).expect_err("a failed microphone open was accepted");
            assert_eq!(error, format!("the microphone could not be opened ({status})"));
            assert_eq!(unsafe { GetHandleInformation(event, &mut flags) }, 0, "event stayed open after status {status}");
        }
        let event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
        assert_ne!(event, 0, "the microphone event could not be created");
        let mut flags = 0u32;
        assert!(reject_failed_wave_open(event, 0).is_ok(), "a successful open status was rejected");
        assert_ne!(unsafe { GetHandleInformation(event, &mut flags) }, 0, "a successful open status closed the event");
        close_wave_event(event);
        assert_eq!(unsafe { GetHandleInformation(event, &mut flags) }, 0, "event stayed open after the success-path cleanup");
        println!("failed_microphone_open_closes_the_event_handle passed");
    }

    #[cfg_attr(test, test)]
    fn native_tools_refuse_paths_outside_the_selected_folder() {
        let root = scratch("confine");
        let selected = root.join("selected");
        let outside = root.join("outside");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        std::fs::write(selected.join("note.txt"), b"note").unwrap();
        let selected_text = path_text(&selected);
        let outside_file = path_text(&outside.join("secret.txt"));
        let escaped = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s(&outside_file))])),
        ]), &selected_text).unwrap_err();
        assert!(escaped.to_ascii_lowercase().contains("selected folder"), "{escaped}");
        assert_eq!(std::fs::read(outside.join("secret.txt")).unwrap(), b"secret");
        let listed = device_folder_tool(&obj(vec![
            ("tool", s("phone_list")),
            ("args", obj(vec![("path", s(&path_text(&outside)))])),
        ]), &selected_text).unwrap_err();
        assert!(listed.to_ascii_lowercase().contains("selected folder"), "{listed}");
        let parent = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("../outside/secret.txt"))])),
        ]), &selected_text).unwrap_err();
        assert!(parent.to_ascii_lowercase().contains("selected folder"), "{parent}");
        let absolute = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s(&path_text(&selected.join("note.txt"))))])),
        ]), &selected_text).unwrap();
        assert_eq!(absolute, "note");
        let relative = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("note.txt"))])),
        ]), &selected_text).unwrap();
        assert_eq!(relative, "note");
        link_directory(&selected.join("link"), &outside).unwrap();
        let linked = device_folder_tool(&obj(vec![
            ("tool", s("phone_read")),
            ("args", obj(vec![("path", s("link/secret.txt"))])),
        ]), &selected_text).unwrap_err();
        assert!(linked.to_ascii_lowercase().contains("selected folder"), "{linked}");
        assert_eq!(std::fs::read(outside.join("secret.txt")).unwrap(), b"secret");
        #[cfg(windows)]
        {
            let drive = device_folder_tool(&obj(vec![
                ("tool", s("phone_list")),
                ("args", obj(vec![("path", s(r"C:\"))])),
            ]), &selected_text).unwrap_err();
            assert!(drive.to_ascii_lowercase().contains("selected folder"), "{drive}");
            let slash = device_folder_tool(&obj(vec![
                ("tool", s("phone_stat")),
                ("args", obj(vec![("path", s("C:/Windows"))])),
            ]), &selected_text).unwrap_err();
            assert!(slash.to_ascii_lowercase().contains("selected folder"), "{slash}");
        }
    }

    fn fill_socket(stream: &mut std::net::TcpStream) {
        stream.set_nonblocking(true).unwrap();
        let chunk = vec![b'x'; 64 * 1024];
        loop {
            match std::io::Write::write(stream, &chunk) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
                Err(error) => panic!("fill: {error}"),
            }
        }
        stream.set_nonblocking(false).unwrap();
    }

    #[cfg_attr(test, test)]
    fn stalled_upload_cannot_outlive_its_deadline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let held = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(3));
            drop(socket);
        });
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        fill_socket(&mut stream);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
        let _watch = arm_write_deadline(&stream, deadline);
        let started = std::time::Instant::now();
        let effect = obj(vec![("wire", s(&"x".repeat(64 * 1024)))]);
        let err = write_request(&mut stream, &effect, deadline).unwrap_err();
        let elapsed = started.elapsed();
        assert!(err.contains("two minutes"), "{err}");
        assert!(elapsed < std::time::Duration::from_secs(2), "upload held the deadline for {elapsed:?}");
        drop(_watch);
        drop(stream);
        let _ = held.join();
    }

    #[cfg_attr(test, test)]
    fn stalled_write_timeout_cannot_restart_the_deadline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let held = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(3));
            drop(socket);
        });
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        fill_socket(&mut stream);
        stream.set_write_timeout(Some(std::time::Duration::from_millis(40))).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        let started = std::time::Instant::now();
        let err = write_all_until(&mut stream, &[b'y'; 64 * 1024], deadline).unwrap_err();
        let elapsed = started.elapsed();
        assert!(err.contains("two minutes"), "{err}");
        assert!(elapsed < std::time::Duration::from_secs(2), "timed-out writes ran for {elapsed:?}");
        assert!(elapsed >= std::time::Duration::from_millis(200), "deadline was not the two-minute budget: {elapsed:?}");
        drop(stream);
        let _ = held.join();
    }

    #[cfg_attr(test, test)]
    fn audio_queue_samples_follow_capacity_not_the_user_pointer() {
        let pointer = std::mem::size_of::<usize>();
        assert_eq!(std::mem::offset_of!(QueueBuffer, capacity), 0);
        assert_eq!(std::mem::offset_of!(QueueBuffer, audio_data), pointer);
        assert_eq!(std::mem::offset_of!(QueueBuffer, byte_size), pointer.saturating_mul(2));
        let user = std::mem::offset_of!(QueueBuffer, user_data);
        assert!(user > std::mem::offset_of!(QueueBuffer, audio_data));
        if pointer == 8 {
            assert_eq!(std::mem::offset_of!(QueueBuffer, audio_data), 8);
            assert_eq!(std::mem::offset_of!(QueueBuffer, byte_size), 16);
            assert_eq!(user, 24);
        }
        let mut samples = [1u8, 2, 3, 4];
        let mut decoy = [9u8, 9, 9, 9];
        let live = QueueBuffer {
            capacity: 4,
            audio_data: samples.as_mut_ptr(),
            byte_size: 4,
            user_data: decoy.as_mut_ptr().cast(),
            packet_description_capacity: 0,
            packet_descriptions: std::ptr::null_mut(),
            packet_description_count: 0,
        };
        let got = unsafe { std::slice::from_raw_parts(live.audio_data, live.byte_size as usize) };
        assert_eq!(got, &[1, 2, 3, 4]);
        assert!(live.audio_data != decoy.as_mut_ptr());
        let abandoned = QueueBuffer {
            capacity: 4,
            audio_data: std::ptr::null_mut(),
            byte_size: 4,
            user_data: decoy.as_mut_ptr().cast(),
            packet_description_capacity: 0,
            packet_descriptions: std::ptr::null_mut(),
            packet_description_count: 0,
        };
        assert!(abandoned.audio_data.is_null());
        assert!(!abandoned.user_data.is_null());
        assert_ne!(std::mem::offset_of!(QueueBuffer, audio_data), std::mem::offset_of!(QueueBuffer, user_data));
    }

    fn scratch_fix6(name: &str) -> PathBuf {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("loops-lanes").join("G1-fix6-desktop-proof").join("tmp").join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg_attr(test, test)]
    fn replaced_file_is_not_deleted() {
        let root = scratch_fix6("replaced-file");
        let dir = root.join("folder");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        let mut original = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        original.write_all(b"ours").unwrap();
        std::fs::rename(&path, dir.join("moved.txt")).unwrap();
        std::fs::write(&path, b"theirs").unwrap();
        assert!(!delete_same_file(&path, &original), "a replaced file was deleted");
        assert_eq!(std::fs::read(&path).unwrap(), b"theirs");
        let replacement = std::fs::File::open(&path).unwrap();
        assert!(delete_same_file(&path, &replacement), "the confirmed file was left in place");
        drop(replacement);
        assert!(!path.exists());
        assert_eq!(std::fs::read(dir.join("moved.txt")).unwrap(), b"ours");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn replaced_directory_entry_is_left_in_place() {
        let root = scratch_fix6("replaced-entry");
        let dir_path = root.join("folder");
        std::fs::create_dir_all(&dir_path).unwrap();
        let path = dir_path.join("note.txt");
        let mut original = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        original.write_all(b"ours").unwrap();
        let dir = std::fs::File::open(&dir_path).unwrap();
        let opened = host_fs::open_at(&dir, b"note.txt", 0x20000).unwrap();
        let id = file_index(&opened).unwrap();
        assert_eq!(file_index(&original), Some(id));
        drop(opened);
        std::fs::rename(&path, dir_path.join("moved.txt")).unwrap();
        std::fs::write(&path, b"theirs").unwrap();
        let name = std::ffi::CString::new("note.txt").unwrap();
        assert!(!unlink_opened_name(&dir, &name, id), "unlink removed the replacement");
        assert_eq!(std::fs::read(&path).unwrap(), b"theirs");
    }

    #[cfg_attr(test, test)]
    fn control_request_on_a_stream_is_acknowledged_and_the_stream_stays_open() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let accept = |listener: &std::net::TcpListener| -> std::io::Result<std::net::TcpStream> {
                listener.set_nonblocking(true)?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
                loop {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream.set_nonblocking(false)?;
                            return Ok(stream);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() >= deadline {
                                return Err(error);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(error) => return Err(error),
                    }
                }
            };
            let mut stream = accept(&listener).expect("stream connection");
            let opening = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nMcp-Session-Id: session\r\n\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n";
            stream.write_all(opening).expect("stream header");
            let mut post = accept(&listener).expect("control connection");
            post.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            let mut wanted = usize::MAX;
            while request.len() < wanted && request.len() < 64 * 1024 {
                match post.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        request.extend_from_slice(&buf[..n]);
                        if wanted == usize::MAX {
                            if let Some(at) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                                let head = String::from_utf8_lossy(&request[..at]);
                                let length = head.lines().find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    if name.eq_ignore_ascii_case("content-length") {
                                        value.trim().parse::<usize>().ok()
                                    } else {
                                        None
                                    }
                                }).unwrap_or(0);
                                wanted = at + 4 + length;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let accepted = b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = post.write_all(accepted);
            let _ = post.shutdown(std::net::Shutdown::Both);
            let _ = stream.write_all(b"data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\",\"params\":{}}\n\n");
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let _ = tx.send(request);
        });
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_millis(200))).ok();
        stream.set_write_timeout(Some(std::time::Duration::from_millis(200))).ok();
        let effect = obj(vec![
            ("stream_protocol", s("mcp_server")),
            ("mcp_control", obj(vec![
                ("endpoint", s(&format!("http://127.0.0.1:{port}/mcp"))),
                ("key", s("")),
                ("version", s("2025-06-18")),
                ("session", s("session")),
                ("owner", s("server-phase")),
                ("request", s("GET")),
                ("request_binding", Num("2".into())),
                ("resumable", Bool(false)),
            ])),
        ]);
        let bytes = read_stream(&mut stream, &effect, &mut |_| {}, std::time::Instant::now() + std::time::Duration::from_secs(8)).expect("control request closed the stream");
        let text = String::from_utf8(bytes).expect("stream result");
        assert!(text.contains("\"requests\":1"), "{text}");
        assert!(text.contains("\"notifications\":1"), "{text}");
        assert!(text.contains("\"messages\":2"), "{text}");
        assert!(!text.contains("not available"), "{text}");
        let posted = rx.recv_timeout(std::time::Duration::from_secs(8)).expect("control request was not sent");
        let posted = String::from_utf8_lossy(&posted);
        assert!(posted.contains("\"result\":{}"), "{posted}");
        server.join().expect("control peer");
    }

    #[cfg_attr(test, test)]
    fn non_windows_file_choice_is_not_unsupported() {
        let root = scratch_fix6("choice");
        let file = root.join("demo.txt");
        std::fs::write(&file, b"demo").unwrap();
        let chosen = interpret_file_line(&path_text(&file), false).unwrap();
        assert_eq!(chosen.as_deref(), Some(path_text(&file).as_str()));
        assert!(interpret_file_line("   ", false).unwrap().is_none());
        let missing = interpret_file_line(&path_text(&root.join("missing.txt")), false).unwrap_err();
        let lower = missing.to_ascii_lowercase();
        assert!(!lower.contains("not available"), "{missing}");
        assert!(!lower.contains("unsupported"), "{missing}");
        let save = interpret_file_line(&path_text(&root.join("notes.txt")), true).unwrap();
        assert_eq!(save.as_deref(), Some(path_text(&root.join("notes.txt")).as_str()));
        #[cfg(not(windows))]
        {
            let picked = choose_or_prompt("Attach image or file", false, "", false, Some(&path_text(&file))).unwrap();
            assert_eq!(picked.as_deref(), Some(path_text(&file).as_str()));
            let closed = choose_or_prompt("Save export", true, "notes.txt", false, Some("")).unwrap();
            assert!(closed.is_none());
            let saved = choose_or_prompt("Save export", true, "notes.txt", false, Some(&path_text(&root.join("notes.txt")))).unwrap();
            assert_eq!(saved.as_deref(), Some(path_text(&root.join("notes.txt")).as_str()));
            let absent = choose_or_prompt("Attach image or file", false, "", false, Some(&path_text(&root.join("missing.txt")))).unwrap_err();
            let lower = absent.to_ascii_lowercase();
            assert!(!lower.contains("not available"), "{absent}");
            assert!(!lower.contains("unsupported"), "{absent}");
        }
    }

    #[cfg(target_os = "linux")]
    #[cfg_attr(test, test)]
    fn portal_response_returns_the_decoded_path() {
        let mut body = Vec::new();
        bus_put_u32(&mut body, 0);
        bus_put_array(&mut body, 8, |buf| {
            bus_pad(buf, 8);
            bus_put_string(buf, "uris");
            bus_put_signature(buf, "as");
            bus_put_array(buf, 4, |arr| {
                bus_put_string(arr, "file:///tmp/a%20b.txt");
            });
        });
        let (code, uris) = portal_uris(&body).unwrap();
        assert_eq!(code, 0);
        assert_eq!(uris, vec!["file:///tmp/a%20b.txt".to_string()]);
        assert_eq!(file_uri_path(&uris[0]).unwrap(), "/tmp/a b.txt");
        assert_eq!(file_uri_path("file://localhost/tmp/a").unwrap(), "/tmp/a");
    }

    #[cfg_attr(test, test)]
    fn typed_multibyte_path_round_trips() {
        let typed = "/home/用户/файл-α/notes.txt";
        let mut input = std::io::Cursor::new(format!("{typed}\r\n").into_bytes());
        let line = read_typed_line(&mut input).expect("typed line");
        assert_eq!(line, typed);
        assert_eq!(line.as_bytes(), typed.as_bytes());
        let root = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix8-desktop-proof")
            .join("tmp")
            .join("typed");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let fresh = path_text(&root.join("用户-файл-α.txt"));
        let mut typed_file = std::io::Cursor::new(format!("{fresh}\n").into_bytes());
        let saved_line = read_typed_line(&mut typed_file).expect("typed file");
        assert_eq!(saved_line, fresh);
        let saved = interpret_file_line(&saved_line, true).expect("save path");
        assert_eq!(saved.as_deref(), Some(fresh.as_str()));
        assert!(!root.join("用户-файл-α.txt").exists());
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_save_target_is_refused_without_blocking() {
        let root = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix8-desktop-proof")
            .join("tmp")
            .join("fifo");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pipe = root.join("export.pipe");
        make_fifo(&pipe).expect("fifo");
        let shown = path_text(&pipe);
        let started = std::time::Instant::now();
        let error = interpret_file_line(&shown, true).expect_err("fifo was accepted as a save path");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "save check blocked on a fifo: {error}");
        assert!(error.contains(&shown), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let regular = root.join("notes.txt");
        std::fs::write(&regular, b"kept").unwrap();
        let saved = interpret_file_line(&path_text(&regular), true).unwrap();
        assert_eq!(saved.as_deref(), Some(path_text(&regular).as_str()));
        assert_eq!(std::fs::read(&regular).unwrap(), b"kept");
        let fresh = root.join("new-notes.txt");
        let created = interpret_file_line(&path_text(&fresh), true).unwrap();
        assert_eq!(created.as_deref(), Some(path_text(&fresh).as_str()));
        assert!(!fresh.exists());
        let folder = root.join("folder");
        std::fs::create_dir(&folder).unwrap();
        let directory = interpret_file_line(&path_text(&folder), true).unwrap_err();
        assert_eq!(directory, "Choose a file path");
    }

    fn import_scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix10-desktop-proof")
            .join("tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg_attr(test, test)]
    fn directory_import_is_refused_before_it_is_opened() {
        let root = import_scratch("dir-import");
        let started = std::time::Instant::now();
        let error = read_text(&path_text(&root), 1024 * 1024).expect_err("importing a directory succeeded");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "import blocked: {error}");
        assert!(error.contains("regular file"), "{error}");
        let backup = root.join("conversation.txt");
        std::fs::write(&backup, "restored conversation").unwrap();
        let text = read_text(&path_text(&backup), 1024 * 1024).expect("regular backup");
        assert_eq!(text, "restored conversation");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_import_returns_an_error_promptly() {
        let root = import_scratch("fifo-import");
        let pipe = root.join("conversation.pipe");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = read_text(&path_text(&pipe), 1024 * 1024).expect_err("importing a fifo succeeded");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "import blocked on a fifo: {error}");
        assert!(error.contains("regular file"), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let backup = root.join("conversation.txt");
        std::fs::write(&backup, "restored conversation").unwrap();
        let text = read_text(&path_text(&backup), 1024 * 1024).expect("regular backup");
        assert_eq!(text, "restored conversation");
    }

    #[cfg(unix)]
    fn fix14_scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix14-desktop-proof")
            .join("tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_phone_write_and_phone_update_return_an_error_promptly() {
        let root = fix14_scratch("phone-write-fifo");
        let pipe = root.join("notes.pipe");
        make_fifo(&pipe).expect("fifo");
        let root_text = path_text(&root);
        let pipe_text = path_text(&pipe);
        let args = obj(vec![("path", s(&pipe_text)), ("content", s("nope"))]);
        let started = std::time::Instant::now();
        let write_error = prove(&root_text, "phone_write", args.clone()).expect_err("phone_write accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_write blocked on a fifo: {write_error}"
        );
        let started = std::time::Instant::now();
        let update_error = prove(&root_text, "phone_update", args).expect_err("phone_update accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_update blocked on a fifo: {update_error}"
        );
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let note = root.join("notes.txt");
        let wrote = prove(
            &root_text,
            "phone_write",
            obj(vec![("path", s(&path_text(&note))), ("content", s("kept"))]),
        )
        .expect("regular phone_write");
        assert!(wrote.contains("kept") || wrote.contains("bytes"), "{wrote}");
        let updated = prove(
            &root_text,
            "phone_update",
            obj(vec![("path", s(&path_text(&note))), ("content", s("revised"))]),
        )
        .expect("regular phone_update");
        assert!(updated.contains("revised") || updated.contains("bytes"), "{updated}");
        let read = prove(&root_text, "phone_read", obj(vec![("path", s(&path_text(&note)))])).expect("regular read");
        assert_eq!(read, "revised");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_phone_revision_returns_an_error_promptly() {
        let root = fix14_scratch("revision-fifo");
        let pipe = root.join("notes.pipe");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = phone_revision(&obj(vec![
            ("tree", s(&path_text(&root))),
            ("path", s("notes.pipe")),
            ("phase", s("read")),
        ]))
        .expect_err("phone_revision accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_revision blocked on a fifo: {error}"
        );
        assert!(error.contains("regular file"), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let note = root.join("notes.txt");
        std::fs::write(&note, b"kept").unwrap();
        let read = phone_revision(&obj(vec![
            ("tree", s(&path_text(&root))),
            ("path", s("notes.txt")),
            ("phase", s("read")),
        ]))
        .expect("regular phone_revision");
        let bytes = network::unbase64(&text(&read, "data")).expect("revision data");
        assert_eq!(bytes, b"kept");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_read_project_file_returns_an_error_promptly() {
        let root = fix14_scratch("project-fifo");
        let pipe = root.join("project.pipe");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = read_project_file(&path_text(&root), "project.pipe").expect_err("read_project_file accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "read_project_file blocked on a fifo: {error}"
        );
        assert!(error.contains("project file") || error.contains("regular"), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        std::fs::write(root.join("project.txt"), b"kept").unwrap();
        let bytes = read_project_file(&path_text(&root), "project.txt")
            .expect("regular project file")
            .expect("project bytes");
        assert_eq!(bytes, b"kept");
    }

    #[cfg(unix)]
    fn lane_scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("loops-lanes")
            .join("G1-fix12-desktop-proof")
            .join("tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_phone_stat_and_phone_read_return_an_error_promptly() {
        let root = lane_scratch("phone-fifo");
        let pipe = root.join("notes.pipe");
        make_fifo(&pipe).expect("fifo");
        let root_text = path_text(&root);
        let pipe_text = path_text(&pipe);
        let started = std::time::Instant::now();
        let stat_error = prove(&root_text, "phone_stat", obj(vec![("path", s(&pipe_text))]))
            .expect_err("phone_stat accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_stat blocked on a fifo: {stat_error}"
        );
        assert!(stat_error.contains("regular file"), "{stat_error}");
        let started = std::time::Instant::now();
        let read_error = prove(&root_text, "phone_read", obj(vec![("path", s(&pipe_text))]))
            .expect_err("phone_read accepted a fifo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "phone_read blocked on a fifo: {read_error}"
        );
        assert!(read_error.contains("regular file"), "{read_error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let folder = parse(&prove(&root_text, "phone_stat", obj(vec![("path", s(""))])).expect("folder stat"))
            .expect("folder stat json");
        assert_eq!(text(&folder, "kind"), "directory");
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let nested_stat = parse(
            &prove(&root_text, "phone_stat", obj(vec![("path", s(&path_text(&nested)))])).expect("directory stat"),
        )
        .expect("directory stat json");
        assert_eq!(text(&nested_stat, "kind"), "directory");
        let note = root.join("notes.txt");
        std::fs::write(&note, b"kept").unwrap();
        let read = prove(&root_text, "phone_read", obj(vec![("path", s(&path_text(&note)))])).expect("regular read");
        assert_eq!(read, "kept");
        let stat = parse(
            &prove(&root_text, "phone_stat", obj(vec![("path", s(&path_text(&note)))])).expect("file stat"),
        )
        .expect("file stat json");
        assert_eq!(text(&stat, "kind"), "file");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn fifo_zip_import_returns_an_error_promptly() {
        let root = lane_scratch("zip-fifo");
        let pipe = root.join("project.pipe");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = read_limited(&path_text(&pipe), PROJECT_FILE_LIMIT).expect_err("importing a fifo succeeded");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "zip import blocked on a fifo: {error}"
        );
        assert!(error.contains("Select a file, not a folder"), "{error}");
        assert_eq!(std::fs::metadata(&pipe).unwrap().len(), 0);
        let archive = root.join("project.bin");
        std::fs::write(&archive, b"PK\x03\x04kept").unwrap();
        let bytes = read_limited(&path_text(&archive), PROJECT_FILE_LIMIT).expect("regular archive");
        assert_eq!(bytes, b"PK\x03\x04kept");
    }

    #[cfg(unix)]
    fn make_fifo(path: &Path) -> std::io::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        type FifoMode = u16;
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        type FifoMode = u32;
        unsafe extern "C" {
            fn mkfifo(path: *const std::ffi::c_char, mode: FifoMode) -> i32;
        }
        if unsafe { mkfifo(c_path.as_ptr(), 0o600) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    fn fix18_scratch(name: &str) -> PathBuf {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("loops-lanes").join("G1-fix18-desktop-proof").join("tmp").join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct RestoreEnv {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl RestoreEnv {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn selected_workspace(name: &str) -> (PathBuf, Json) {
        let root = fix18_scratch(name);
        let root_text = path_text(&root);
        let opening = core_event(&Null, &root_text, "resume", &Null).expect("resume");
        let mut state = opening.get("state").expect("resume state").clone();
        advance(&mut state, &root_text, "workspace_select:default", Null).expect("workspace");
        assert!(!text(&state, "approval_root").is_empty(), "workspace was not selected");
        (root, state)
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn key_fifo_is_refused_without_blocking() {
        let root = fix18_scratch("key-fifo");
        let pipe = root.join("key");
        make_fifo(&pipe).expect("fifo");
        let started = std::time::Instant::now();
        let error = read_key(&pipe).expect_err("fifo key was accepted");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "key open blocked: {error}");
        assert!(error.contains("regular file"), "{error}");
        let created = root.join("fresh-key");
        let key = read_key(&created).expect("fresh key");
        assert_eq!(std::fs::read(&created).unwrap(), key);
        assert_eq!(read_key(&created).unwrap(), key);
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn state_fifo_load_and_save_are_refused_without_blocking() {
        let root = fix18_scratch("state-fifo");
        let kept = root.join("kept.bin");
        std::fs::write(&kept, b"previous-valid-state").unwrap();
        let pipe = root.join("state.bin");
        make_fifo(&pipe).expect("fifo");
        let vault = Vault { path: pipe, key: [3u8; 32] };
        let started = std::time::Instant::now();
        let load_error = vault.load().expect_err("fifo load was accepted");
        let save_error = vault.save(&obj(vec![("kept", s("next"))])).expect_err("fifo save was accepted");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "state open blocked: {load_error} {save_error}");
        assert!(load_error.contains("regular file"), "{load_error}");
        assert!(save_error.contains("regular file"), "{save_error}");
        assert_eq!(std::fs::read(&kept).unwrap(), b"previous-valid-state");
    }

    #[cfg_attr(test, test)]
    fn vault_save_replaces_the_previous_file_after_the_new_bytes_are_written() {
        use crate::host_fs::MetadataExt;
        let root = fix18_scratch("vault");
        let path = root.join("state.bin");
        std::fs::write(&path, b"previous-valid-state").unwrap();
        let before_id = std::fs::metadata(&path).unwrap().ino();
        let vault = Vault { path: path.clone(), key: [9u8; 32] };
        vault.save(&obj(vec![("kept", s("next"))])).expect("save");
        let after = std::fs::metadata(&path).unwrap();
        assert_ne!(before_id, after.ino(), "save truncated the existing vault in place");
        assert_ne!(std::fs::read(&path).unwrap(), b"previous-valid-state");
        assert!(after.len() > 16);
        let loaded = vault.load().expect("load").expect("saved state missing");
        assert_eq!(text(&loaded, "kept"), "next");
        vault.save(&obj(vec![("kept", s("later"))])).expect("second save");
        let loaded = vault.load().expect("reload").expect("saved state missing");
        assert_eq!(text(&loaded, "kept"), "later");
        let missing = Vault { path: root.join("absent.bin"), key: [9u8; 32] };
        assert!(missing.load().expect("missing vault").is_none());
        let created = root.join("key");
        let key = read_key(&created).expect("fresh key");
        assert_eq!(std::fs::read(&created).unwrap(), key);
    }

    #[cfg(windows)]
    #[cfg_attr(test, test)]
    fn directory_lock_covers_a_directory_and_a_file_still_locks() {
        use crate::host_fs::OpenOptionsExt;
        let root = fix18_scratch("dirlock");
        let first = std::fs::OpenOptions::new().read(true).custom_flags(131072 | 65536).open(&root).expect("directory");
        first.try_lock().expect("directory lock");
        let second = std::fs::OpenOptions::new().read(true).custom_flags(131072 | 65536).open(&root).expect("second directory");
        let blocked = second.try_lock().expect_err("second directory lock was granted");
        assert!(matches!(blocked, std::fs::TryLockError::WouldBlock), "{blocked:?}");
        drop(first);
        second.try_lock().expect("directory lock after release");
        let path = root.join("plain.bin");
        std::fs::write(&path, b"abc").unwrap();
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        file.try_lock().expect("file lock");
        let other = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        let blocked = other.try_lock().expect_err("second file lock was granted");
        assert!(matches!(blocked, std::fs::TryLockError::WouldBlock), "{blocked:?}");
    }

    #[cfg_attr(test, test)]
    fn media_pick_stores_the_selected_file() {
        let _hold = env_guard();
        let (root, mut state) = selected_workspace("mediawork");
        let root_text = path_text(&root);
        {
            let _empty = RestoreEnv::set("RLLM_MEDIA_FILE", "");
            let ui = advance(&mut state, &root_text, "media_pick", Null).expect("cancel pick");
            assert!(!ui.string().contains("not available on this host"), "{}", ui.string());
            assert!(text(&state, "notice").contains("cancelled"), "{}", text(&state, "notice"));
        }
        let source = root.join("clip.bin");
        std::fs::write(&source, b"hello-media").unwrap();
        let _file = RestoreEnv::set("RLLM_MEDIA_FILE", &source);
        let ui = advance(&mut state, &root_text, "media_pick", Null).expect("pick");
        assert!(!ui.string().contains("not available on this host"), "{}", ui.string());
        let record = get(&state, "media_input").clone();
        assert_ne!(record, Null, "selected file was not stored; notice={} dialog={}", text(&state, "notice"), text(&state, "dialog"));
        let leaf = text(&record, "path").rsplit(['/', '\\']).next().unwrap().to_string();
        let stored = Path::new(&text(&state, "approval_root")).join("service-files").join(leaf);
        assert_eq!(std::fs::read(&stored).expect("stored media"), b"hello-media");
        assert!(text(&state, "notice").contains("Input saved locally"), "{}", text(&state, "notice"));
    }

    #[cfg_attr(test, test)]
    fn media_record_start_and_stop_store_audio() {
        let _hold = env_guard();
        let (root, mut state) = selected_workspace("recordwork");
        let root_text = path_text(&root);
        let idle = advance(&mut state, &root_text, "media_record_stop", Null).expect("idle stop");
        assert!(!idle.string().contains("not available on this host"), "{}", idle.string());
        let mut pcm = Vec::new();
        for _ in 0..640 {
            pcm.extend_from_slice(&[0x11, 0x22]);
        }
        let source = root.join("utterance.pcm");
        std::fs::write(&source, &pcm).unwrap();
        let _source = RestoreEnv::set("RLLM_VOICE_SOURCE", &source);
        let started = advance(&mut state, &root_text, "media_record", Null).expect("record");
        assert!(!started.string().contains("not available on this host"), "{}", started.string());
        assert_eq!(text(&state, "dialog"), "media_capture", "{}", text(&state, "notice"));
        let stopped = advance(&mut state, &root_text, "media_record_stop", Null).expect("stop");
        assert!(!stopped.string().contains("not available on this host"), "{}", stopped.string());
        let record = get(&state, "media_input").clone();
        assert_eq!(text(&record, "mime"), "audio/wav", "{}", record.string());
        let leaf = text(&record, "path").rsplit(['/', '\\']).next().unwrap().to_string();
        let stored = Path::new(&text(&state, "approval_root")).join("service-files").join(leaf);
        let bytes = std::fs::read(&stored).unwrap_or_else(|error| panic!("stored audio: {error}"));
        assert!(bytes.starts_with(b"RIFF"));
        assert!(bytes.windows(pcm.len()).any(|window| window == pcm.as_slice()), "captured samples were not stored");
    }

    #[cfg(unix)]
    #[cfg_attr(test, test)]
    fn media_pick_fifo_is_refused_without_blocking() {
        let _hold = env_guard();
        let (root, mut state) = selected_workspace("pickfifo");
        let pipe = root.join("clip.pipe");
        make_fifo(&pipe).expect("fifo");
        let _file = RestoreEnv::set("RLLM_MEDIA_FILE", &pipe);
        let started = std::time::Instant::now();
        let ui = advance(&mut state, &path_text(&root), "media_pick", Null).expect("pick");
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "media pick blocked");
        assert!(!ui.string().contains("not available on this host"), "{}", ui.string());
        assert!(text(&state, "notice").contains("unavailable"), "{}", text(&state, "notice"));
    }

    #[cfg_attr(test, test)]
    fn authorization_address_rejects_a_non_http_target() {
        let error = open_authorization("file:///local/secret").expect_err("non-http address was accepted");
        assert!(error.contains("not usable"), "{error}");
        let error = open_authorization("").expect_err("empty address was accepted");
        assert!(error.contains("not usable"), "{error}");
    }

    #[cfg_attr(test, test)]
    fn oauth_browser_keeps_the_sign_in_open() {
        let _hold = env_guard();
        let root = fix18_scratch("oauthwork");
        let root_text = path_text(&root);
        let opening = core_event(&Null, &root_text, "resume", &Null).expect("resume");
        let mut state = opening.get("state").expect("resume state").clone();
        state = set_field(state, "profiles", Arr(vec![obj(vec![
            ("name", s("Loopback")),
            ("model", s("local")),
            ("endpoint", s("http://127.0.0.1:9/v1/chat/completions")),
            ("plain", Bool(true)),
            ("protocol", s("chat")),
        ])]));
        state = set_field(state, "model", s("Loopback"));
        state = set_field(state, "selected", Num("0".into()));
        advance(&mut state, &root_text, "oauth_settings", Null).expect("settings");
        let saved = obj(vec![
            ("authorization_endpoint", s("http://127.0.0.1:9/authorize")),
            ("token_endpoint", s("http://127.0.0.1:9/token")),
            ("client_id", s("desktop-client")),
            ("scope", s("open")),
            ("plain", Bool(true)),
        ]);
        advance(&mut state, &root_text, "oauth_save", saved.clone()).expect("save authorization");
        let capture = root.join("authorization.txt");
        let _capture = RestoreEnv::set("RLLM_AUTHORIZATION_CAPTURE", &capture);
        let ui = advance(&mut state, &root_text, "oauth_sign_in", saved).expect("sign in");
        assert!(!ui.string().contains("not available on this host"), "{}", ui.string());
        assert_eq!(text(&state, "dialog"), "oauth_wait", "{}", text(&state, "notice"));
        let opened = std::fs::read_to_string(&capture).expect("authorization address was not opened");
        assert!(opened.starts_with("http://127.0.0.1:9/authorize?"), "{opened}");
        assert!(opened.contains("client_id=desktop-client"), "{opened}");
        assert!(opened.contains("response_type=code"), "{opened}");
    }

    #[cfg(g1_regression)]
    pub fn fix18() {
        #[cfg(unix)]
        key_fifo_is_refused_without_blocking();
        #[cfg(unix)]
        state_fifo_load_and_save_are_refused_without_blocking();
        vault_save_replaces_the_previous_file_after_the_new_bytes_are_written();
        #[cfg(windows)]
        directory_lock_covers_a_directory_and_a_file_still_locks();
        media_pick_stores_the_selected_file();
        media_record_start_and_stop_store_audio();
        #[cfg(unix)]
        media_pick_fifo_is_refused_without_blocking();
        oauth_browser_keeps_the_sign_in_open();
        authorization_address_rejects_a_non_http_target();
        println!("g1-fix18 passed");
    }

    #[cfg(g1_regression)]
    pub fn run() {
        #[cfg(windows)]
        failed_microphone_open_closes_the_event_handle();
        revision_rejects_a_linked_parent();
        revision_reads_binary_past_one_mebibyte();
        limited_read_stops_before_a_declared_huge_file();
        failed_project_write_preserves_the_handle_and_not_a_replacement();
        stream_deadline_does_not_restart_when_bytes_arrive();
        empty_capture_is_not_a_fatal_read();
        session_hello_matches_the_published_core_method();
        session_negotiation_returns_samples_only_after_a_capture_buffer();
        nested_format_rate_is_recognized_and_a_hostile_size_is_not();
        stdin_line_stops_at_the_parse_limit();
        swapped_directory_entry_cannot_escape_the_selected_folder();
        in_folder_link_still_reads_the_target_file();
        #[cfg(windows)]
        wave_header_matches_the_published_record();
        native_tools_refuse_paths_outside_the_selected_folder();
        stalled_upload_cannot_outlive_its_deadline();
        stalled_write_timeout_cannot_restart_the_deadline();
        audio_queue_samples_follow_capacity_not_the_user_pointer();
        replaced_file_is_not_deleted();
        #[cfg(unix)]
        replaced_directory_entry_is_left_in_place();
        control_request_on_a_stream_is_acknowledged_and_the_stream_stays_open();
        non_windows_file_choice_is_not_unsupported();
        typed_multibyte_path_round_trips();
        directory_import_is_refused_before_it_is_opened();
        #[cfg(unix)]
        fifo_save_target_is_refused_without_blocking();
        #[cfg(unix)]
        fifo_import_returns_an_error_promptly();
        #[cfg(unix)]
        fifo_phone_stat_and_phone_read_return_an_error_promptly();
        #[cfg(unix)]
        fifo_zip_import_returns_an_error_promptly();
        #[cfg(unix)]
        fifo_phone_write_and_phone_update_return_an_error_promptly();
        #[cfg(unix)]
        fifo_phone_revision_returns_an_error_promptly();
        #[cfg(unix)]
        fifo_read_project_file_returns_an_error_promptly();
        #[cfg(target_os = "linux")]
        portal_response_returns_the_decoded_path();
        #[cfg(unix)]
        key_fifo_is_refused_without_blocking();
        #[cfg(unix)]
        state_fifo_load_and_save_are_refused_without_blocking();
        vault_save_replaces_the_previous_file_after_the_new_bytes_are_written();
        #[cfg(windows)]
        directory_lock_covers_a_directory_and_a_file_still_locks();
        media_pick_stores_the_selected_file();
        media_record_start_and_stop_store_audio();
        #[cfg(unix)]
        media_pick_fifo_is_refused_without_blocking();
        oauth_browser_keeps_the_sign_in_open();
        authorization_address_rejects_a_non_http_target();
        println!("g1-regression passed");
    }
}


// Regression dispatch formerly inside main in core/desktop.rs.
    #[cfg(g1_regression)]
    {
        if std::env::var_os("RLLM_FIX18").is_some() {
            desktop_regression::fix18();
        } else {
            desktop_regression::run();
        }
        return;
    }
