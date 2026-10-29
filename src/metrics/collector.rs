use super::*;
use std::{collections::HashMap, sync::mpsc::{self, Receiver}, time::{Duration,Instant}};
use sysinfo::{System,ProcessesToUpdate,ProcessRefreshKind,UpdateKind,Networks,Disks,Users};

pub fn spawn_collector() -> Receiver<Sample> {
    let (tx,rx)=mpsc::sync_channel(2);
    std::thread::spawn(move || {
        let result = collect(&tx);
        if let Err(e)=result { let _=tx.send(Sample{error:Some(format!("Collector stopped: {e}")),..Sample::default()}); }
    });rx
}
fn collect(tx:&mpsc::SyncSender<Sample>)->anyhow::Result<()> {
    let mut sys=System::new_all()?;
    let mut nets=Networks::new_with_refreshed_list()?;
    let mut disks=Disks::new_with_refreshed_list()?;
    let users=Users::new_with_refreshed_list()?;
    let start=Instant::now();
    let mut previous=Instant::now();
    let mut strings:HashMap<(u32,u64),(Arc<str>,Arc<str>,Arc<str>,Arc<str>)>=HashMap::new();
    let mut processes=Arc::new(Vec::new());
    let mut process_at=0.;
    let mut last_process=Instant::now()-Duration::from_secs(20);
    let mut battery=Avail::Acquiring;
    let mut last_battery=Instant::now()-Duration::from_secs(20);
    #[cfg(all(target_os="macos",target_arch="aarch64"))]
    let mut sampler=macmon::Sampler::new().ok();
    loop {
        let began=Instant::now();
        let mut s=Sample::default();
        #[cfg(all(target_os="macos",target_arch="aarch64"))]
        if let Some(sampler)=sampler.as_mut() {
            match sampler.get_metrics(1000) {
                Ok(m)=>{
                    let info=sampler.get_soc_info();
                    s.machine=Arc::new(Machine{chip:info.chip_name.clone(),model:info.mac_model.clone(),gpu_cores:info.gpu_cores as usize,memory:m.memory.ram_total,cpu_count:info.cpu_tiers.iter().map(|t|t.cores as usize).sum()});
                    s.cpu=m.cpu_active_ratio;
                    s.tiers=m.cpu_tiers.iter().map(|t|Tier{label:t.label.clone(),mhz:t.freq_mhz,max_mhz:info.cpu_tiers.iter().find(|i|i.label==t.label).and_then(|i|i.freqs.last()).copied().unwrap_or(1),active:t.active_ratio,scaled:t.scaled_ratio,cores:t.cores.iter().map(|c|Core{id:c.core_id,die:c.die_id,active:c.active_ratio,scaled:c.scaled_ratio,mhz:c.freq_mhz}).collect()}).collect();
                    s.tiers.sort_by_key(|t|std::cmp::Reverse(t.cores.len()));
                    s.gpu=Avail::Ok(m.gpu_active_ratio);s.gpu_mhz=Avail::Ok(m.gpu_freq_mhz);
                    s.power=[m.cpu_power,m.gpu_power,m.ane_power,m.ram_power,m.gpu_ram_power].map(|v|if v.is_finite()&&v>=0. {Avail::Ok(v)}else{Avail::Failed("invalid power counter".into())});
                    s.system_power=normalize_system_power(m.sys_power);
                    s.cpu_temp=m.temp.cpu_temp_avg.map(Avail::Ok).unwrap_or(Avail::Unsupported("CPU temperature not exposed".into()));
                    s.gpu_temp=m.temp.gpu_temp_avg.map(Avail::Ok).unwrap_or(Avail::Unsupported("GPU temperature not exposed".into()));
                    s.fans=if m.fans.is_empty() {
                        if info.chip_name.contains("Air")||info.mac_model.contains("Mac14,2") {Avail::NoHardware("passive cooling".into())} else {Avail::Unsupported("no fan sensors reported".into())}
                    } else {Avail::Ok(m.fans.iter().map(|f|Fan{name:f.name.clone(),rpm:f.rpm,max:f.max_rpm}).collect())};
                    if info.vm {s.power=std::array::from_fn(|_|Avail::Unsupported("virtual machine".into()));s.gpu=Avail::Unsupported("virtual machine".into());}
                    s.memory_used=m.memory.ram_usage;s.swap=m.memory.swap_usage;
                },
                Err(e)=>s.error=Some(format!("Hardware sampler: {e}")),
            }
        }
        if s.machine.chip.is_empty() {
            std::thread::sleep(Duration::from_secs(1).saturating_sub(began.elapsed()));
            sys.refresh_cpu_usage();sys.refresh_memory();
            s.machine=Arc::new(Machine{chip:sys.cpus().first().map(|c|c.brand().to_owned()).unwrap_or("System CPU".into()),model:"Generic system".into(),gpu_cores:0,memory:sys.total_memory(),cpu_count:sys.cpus().len()});
            s.cpu=sys.global_cpu_usage()/100.;s.memory_used=sys.used_memory();s.swap=sys.used_swap();
            s.tiers=vec![Tier{label:"CPU".into(),mhz:0,max_mhz:1,active:s.cpu,scaled:s.cpu,cores:sys.cpus().iter().enumerate().map(|(i,c)|Core{id:i,die:0,active:c.usage()/100.,scaled:c.usage()/100.,mhz:0}).collect()}];
            let na=||Avail::Unsupported("hardware sensor not exposed".into());
            s.power=std::array::from_fn(|_|na());s.system_power=na();s.cpu_temp=na();s.gpu_temp=na();s.gpu=na();s.gpu_mhz=Avail::Unsupported("clock unavailable".into());s.fans=Avail::Unsupported("fan sensors unavailable".into());
        }
        s.at=start.elapsed().as_secs_f64();
        let elapsed=previous.elapsed().as_secs_f64().max(0.001);previous=Instant::now();
        nets.refresh(true);disks.refresh(true);
        // One primary active physical interface; avoid counting loopback and VPN copies twice.
        if let Some((name,n))=nets.iter().filter(|(n,_)|n.to_string_lossy().starts_with("en")||n.to_string_lossy().starts_with("eth")||n.to_string_lossy().starts_with("wl")).max_by_key(|(_,n)|n.received()+n.transmitted()) {
            s.network=Avail::Ok(Flow{read:n.received() as f64/elapsed,write:n.transmitted() as f64/elapsed,packets_in:n.packets_received() as f64/elapsed,packets_out:n.packets_transmitted() as f64/elapsed,errors:n.errors_on_received()+n.errors_on_transmitted(),name:name.to_string_lossy().into_owned()});
        }else{s.network=Avail::Unsupported("no active physical interface".into());}
        // Use the root volume only; APFS volumes share physical counters/capacity.
        if let Some(d)=disks.iter().find(|d|d.mount_point()==std::path::Path::new("/")) {
            let u=d.usage();s.disk=Avail::Ok(Flow{read:u.read_bytes as f64/elapsed,write:u.written_bytes as f64/elapsed,name:d.name().to_string_lossy().into(),..Flow::default()});
            s.disk_capacity=Avail::Ok((d.total_space().saturating_sub(d.available_space()),d.total_space()));
        } else {s.disk=Avail::Unsupported("root disk counters unavailable".into());s.disk_capacity=Avail::Unsupported("capacity unavailable".into());}
        if last_process.elapsed()>=Duration::from_secs(5) {
            let dt=last_process.elapsed().as_secs_f64();
            sys.refresh_processes_specifics(ProcessesToUpdate::All,true,ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage().with_cmd(UpdateKind::OnlyIfNotSet).with_user(UpdateKind::OnlyIfNotSet));
            let mut out=Vec::with_capacity(sys.processes().len());
            for (pid,p) in sys.processes() {
                let key=(pid.as_u32(),p.start_time());
                let meta=strings.entry(key).or_insert_with(||(
                    Arc::from(p.name().to_string_lossy().as_ref()),
                    Arc::from(p.cmd().iter().map(|s|s.to_string_lossy()).collect::<Vec<_>>().join(" ")),
                    Arc::from(p.user_id().and_then(|uid|users.get_user_by_id(uid)).map(|u|u.name()).unwrap_or("unknown")),
                    Arc::from(format!("{:?}",p.status()))));
                let u=p.disk_usage();
                out.push(Proc{pid:pid.as_u32(),start:p.start_time(),parent:p.parent().map(|p|p.as_u32()).unwrap_or(0),name:meta.0.clone(),command:meta.1.clone(),user:meta.2.clone(),cpu:p.cpu_usage(),memory:p.memory(),read:u.read_bytes as f64/dt,write:u.written_bytes as f64/dt,runtime:p.run_time(),status:meta.3.clone()});
            }
            strings.retain(|(pid,start),_|sys.process(sysinfo::Pid::from_u32(*pid)).is_some_and(|p|p.start_time()==*start));
            out.sort_by_key(|p|p.pid);processes=Arc::new(out);process_at=s.at;last_process=Instant::now();
        }
        s.processes=processes.clone();s.process_at=process_at;
        if last_battery.elapsed()>=Duration::from_secs(10) {battery=read_battery();last_battery=Instant::now();}
        s.battery=battery.clone();
        s.uptime=System::uptime().unwrap_or(0);
        read_vm(&mut s);
        s.thermal=thermal();
        s.collector_ms=began.elapsed().as_secs_f64()*1000.;
        if tx.send(s).is_err() {return Ok(());}
    }
}
#[cfg(all(target_os="macos",target_arch="aarch64"))]
fn thermal()->Avail<String>{
    use objc2_foundation::{NSProcessInfo,NSProcessInfoThermalState as T};
    let state=NSProcessInfo::processInfo().thermalState();
    Avail::Ok(match state {T::Nominal=>"NOMINAL",T::Fair=>"FAIR",T::Serious=>"SERIOUS",T::Critical=>"CRITICAL",_=>"UNKNOWN"}.into())
}
#[cfg(not(all(target_os="macos",target_arch="aarch64")))]
fn thermal()->Avail<String>{Avail::Unsupported("thermal state unavailable".into())}
fn read_vm(s:&mut Sample) {
    #[cfg(target_os="macos")]
    {
        if let Ok(o)=std::process::Command::new("/usr/bin/vm_stat").output() {
            let text=String::from_utf8_lossy(&o.stdout);
            let page=text.lines().next().and_then(|l|l.split("page size of ").nth(1)).and_then(|p|p.split_whitespace().next()).and_then(|p|p.parse::<u64>().ok()).unwrap_or(16384);
            let count=|key:&str|text.lines().find(|l|l.starts_with(key)).and_then(|l|l.split(':').nth(1)).and_then(|v|v.trim().trim_end_matches('.').parse::<u64>().ok()).map(|n|n*page);
            if let (Some(wired),Some(comp),Some(free))=(count("Pages wired down:"),count("Pages occupied by compressor:"),count("Pages free:")) {
                let used=s.memory_used.min(s.machine.memory);
                let wired=wired.min(used);let comp=comp.min(used-wired);
                let free=free.min(s.machine.memory-used);
                s.memory=Avail::Estimated([wired,used-wired-comp,comp,s.machine.memory-used-free,free]);
            }
        }
        s.pressure=std::process::Command::new("/usr/sbin/sysctl").args(["-n","kern.memorystatus_vm_pressure_level"]).output().ok().and_then(|o|String::from_utf8(o.stdout).ok()).and_then(|s|match s.trim(){"1"=>Some("NORMAL"),"2"=>Some("WARN"),"4"=>Some("CRITICAL"),_=>None}).map(|s|Avail::Ok(s.into())).unwrap_or(Avail::Unsupported("memory pressure unavailable".into()));
    }
    if matches!(s.memory,Avail::Acquiring){s.memory=Avail::Unsupported("VM categories unavailable".into());}
}
#[cfg(all(target_os="macos",target_arch="aarch64"))]
fn read_battery()->Avail<Battery>{
    let Ok(o)=std::process::Command::new("/usr/sbin/ioreg").args(["-a","-r","-c","AppleSmartBattery"]).output() else {return Avail::Failed("battery query failed".into());};
    let Ok(v)=plist::Value::from_reader(std::io::Cursor::new(o.stdout)) else {return Avail::Failed("battery data unreadable".into());};
    let Some(d)=v.as_array().and_then(|a|a.first()).and_then(|v|v.as_dictionary()) else {return Avail::NoHardware("AC powered".into());};
    let n=|k:&str|d.get(k).and_then(|v|v.as_signed_integer().map(|v|v as f64).or_else(||v.as_unsigned_integer().map(|v|v as f64)));
    let b=|k:&str|d.get(k).and_then(|v|v.as_boolean()).unwrap_or(false);
    let Some(current)=n("CurrentCapacity") else{return Avail::Failed("battery capacity unreadable".into());};
    let max=n("MaxCapacity").unwrap_or(100.).max(1.);
    let watts=match(n("Voltage"),n("Amperage")){(Some(v),Some(a))=>{let signed=if a>i64::MAX as f64 {(a as u64 as i64) as f64}else{a};Avail::Ok((v*signed/1_000_000.) as f32)},_=>Avail::Unsupported("pack power unavailable".into())};
    let health=match(n("AppleRawMaxCapacity"),n("DesignCapacity")){(Some(a),Some(b))if b>0.=>Avail::Estimated((a/b*100.).min(100.) as f32),_=>Avail::Unsupported("health unavailable".into())};
    Avail::Ok(Battery{percent:(current/max*100.).clamp(0.,100.) as f32,charging:b("IsCharging"),ac:b("ExternalConnected"),watts,health,cycles:n("CycleCount").map(|n|Avail::Ok(n as u32)).unwrap_or(Avail::Unsupported("cycles unavailable".into())),minutes:n("TimeRemaining").filter(|n|*n>=0.&&*n<65535.).map(|n|Avail::Estimated(n as u32)).unwrap_or(Avail::Unsupported("time unavailable".into()))})
}
#[cfg(not(all(target_os="macos",target_arch="aarch64")))]
fn read_battery()->Avail<Battery>{Avail::Unsupported("battery collector requires Apple Silicon".into())}

/// Revalidate process identity immediately before signalling; never elevate privileges.
pub fn send_signal(p:&Proc,force:bool)->anyhow::Result<()> {
    anyhow::ensure!(p.pid>1 && p.pid!=std::process::id() && p.name.as_ref()!="kernel_task", "This process is protected");
    let mut s=System::new()?;
    let pid=sysinfo::Pid::from_u32(p.pid);
    s.refresh_processes(ProcessesToUpdate::Some(&[pid]),true);
    let live=s.process(pid).ok_or_else(||anyhow::anyhow!("Process has exited"))?;
    anyhow::ensure!(live.start_time()==p.start,"PID was reused; no signal sent");
    let signal=if force {sysinfo::Signal::Kill}else{sysinfo::Signal::Term};
    anyhow::ensure!(live.kill_with(signal)==Some(true),"Signal failed: permission denied or process exited");Ok(())
}
