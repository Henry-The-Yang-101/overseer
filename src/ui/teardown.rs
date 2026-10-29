use ratatui::{buffer::Buffer,layout::Rect,style::Style,symbols::border,widgets::{Block,Widget,BorderType}};
use crate::app::{App,Part};
use super::{SizeClass,kit::*,board,theme::Theme};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Provenance{Photo,Approximate,Conceptual}
#[derive(Clone,Copy,Debug)]pub struct NRect{pub x:f32,pub y:f32,pub w:f32,pub h:f32}
impl NRect{pub const fn new(x:f32,y:f32,w:f32,h:f32)->Self{Self{x,y,w,h}}}
#[derive(Clone,Copy)]pub struct Component{pub part:Part,pub label:&'static str,pub rect:NRect,pub provenance:Provenance}
pub const COMPONENTS:&[Component]=&[
    Component{part:Part::Network,label:"ANTENNA",rect:NRect::new(0.34,0.,0.32,0.05),provenance:Provenance::Photo},
    Component{part:Part::Fan0,label:"FAN 0",rect:NRect::new(0.08,0.09,0.26,0.37),provenance:Provenance::Photo},
    Component{part:Part::Fan1,label:"FAN 1",rect:NRect::new(0.66,0.09,0.26,0.37),provenance:Provenance::Photo},
    Component{part:Part::Soc,label:"SoC",rect:NRect::new(0.44,0.12,0.12,0.18),provenance:Provenance::Photo},
    Component{part:Part::Memory,label:"MEM ≈",rect:NRect::new(0.35,0.16,0.09,0.18),provenance:Provenance::Conceptual},
    Component{part:Part::Memory,label:"MEM ≈",rect:NRect::new(0.56,0.16,0.09,0.18),provenance:Provenance::Conceptual},
    Component{part:Part::Power,label:"PMU ≈",rect:NRect::new(0.37,0.37,0.08,0.07),provenance:Provenance::Conceptual},
    Component{part:Part::Disk,label:"SSD ≈",rect:NRect::new(0.56,0.37,0.08,0.07),provenance:Provenance::Approximate},
    Component{part:Part::Battery,label:"BMS",rect:NRect::new(0.47,0.50,0.06,0.36),provenance:Provenance::Photo},
];
pub const CELLS:[NRect;6]=[NRect::new(0.06,0.50,0.19,0.19),NRect::new(0.06,0.71,0.19,0.19),NRect::new(0.27,0.50,0.20,0.33),NRect::new(0.53,0.50,0.20,0.33),NRect::new(0.75,0.50,0.19,0.19),NRect::new(0.75,0.71,0.19,0.19)];
pub fn fit_chassis(stage:Rect,aspect:f32,cell_aspect:f32)->Rect{let aspect=aspect.max(0.1)*cell_aspect.max(0.1);let h=(stage.width as f32/aspect).min(stage.height as f32).floor()as u16;let w=(h as f32*aspect).floor().min(stage.width as f32)as u16;Rect::new(stage.x+(stage.width-w)/2,stage.y+(stage.height-h)/2,w,h)}
pub fn to_cells(n:NRect,c:Rect,mirror:bool)->Rect{let x=if mirror{1.-n.x-n.w}else{n.x};let l=(x*c.width as f32).round().clamp(0.,c.width as f32)as u16;let r=((x+n.w)*c.width as f32).round().clamp(0.,c.width as f32)as u16;let top=(n.y*c.height as f32).round().clamp(0.,c.height as f32)as u16;let bottom=((n.y+n.h)*c.height as f32).round().clamp(0.,c.height as f32)as u16;Rect::new(c.x+l,c.y+top,r.saturating_sub(l),bottom.saturating_sub(top))}
const DASHED:border::Set=border::Set{top_left:"┌",top_right:"┐",bottom_left:"└",bottom_right:"┘",vertical_left:"┆",vertical_right:"┆",horizontal_top:"┄",horizontal_bottom:"┄"};
const DOTTED:border::Set=border::Set{top_left:"┌",top_right:"┐",bottom_left:"└",bottom_right:"┘",vertical_left:"┊",vertical_right:"┊",horizontal_top:"┈",horizontal_bottom:"┈"};
fn outline(b:&mut Buffer,r:Rect,label:&str,p:Provenance,color:ratatui::style::Color,t:&Theme)->Rect{if r.width<3||r.height<2{line(b,r,0,label,color);return r;}let set=match p{Provenance::Photo=>border::ROUNDED,Provenance::Approximate=>DASHED,Provenance::Conceptual=>DOTTED};let block=Block::bordered().border_set(set).border_style(Style::new().fg(color)).style(Style::new().bg(t.panel)).title_top(format!(" {label} "));let inside=block.inner(r);block.render(r,b);inside}
pub fn draw(b:&mut Buffer,r:Rect,a:&mut App,size:SizeClass){
    let t=a.theme.clone();let sample=a.sample();let verified=sample.machine.model=="Mac16,8";let sidebar=if size==SizeClass::Wide{36}else{0};
    let content=Rect::new(r.x,r.y,r.width.saturating_sub(sidebar+if sidebar>0{1}else{0}),r.height);
    let title=if verified{"MACBOOK PRO 14 / M4 PRO"}else{"GENERIC SCHEMATIC / NOT A PHYSICAL LAYOUT"};
    let inside=frame(b,content,title,if a.cells{"TELEMETRY [m]"}else{"PHYSICAL [m]"},false,&t);
    let orientation=if a.cfg.teardown.orientation=="xray"{"X-RAY VIEW · hinge ↑ · MagSafe ← · HDMI/SD →"}else{"UNDERSIDE VIEW · hinge ↑ · HDMI/SD ← · MagSafe →"};
    line(b,inside,0,if verified{orientation}else{"Schematic arrangement · hardware positions are not known"},t.muted);
    let available=Rect::new(inside.x,inside.y+2,inside.width,inside.height.saturating_sub(4));let chassis=fit_chassis(available,312.6/221.2,a.cell_aspect);
    if chassis.width<64||chassis.height<22{navigator(b,available,a);}
    else if !verified {generic(b,available,a);}else{physical(b,chassis,a);}
    let disclaimer=if a.cells{"CELLS ARE DISPLAY SLOTS · NOT PER-CELL BATTERY DATA"}else if verified{"── photo-matched   ┄┄ approximate   ┈┈ conceptual   Fan sides assumed"}else{"GENERIC SCHEMATIC · NOT THE PHYSICAL LAYOUT OF THIS MACHINE"};
    line(b,inside,inside.height.saturating_sub(1),disclaimer,t.muted);
    if sidebar>0 {let rail=Rect::new(content.right()+1,r.y,sidebar,r.height);selected(b,rail,a);}
}
fn physical(b:&mut Buffer,c:Rect,a:&mut App){let t=a.theme.clone();let s=a.sample();let mirror=a.cfg.teardown.orientation=="xray";let ascii=a.cfg.ui.glyphs=="ascii";let large=c.width>=100&&c.height>=34;
    Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(t.wire)).render(c,b);
    let board_y=(c.height as f32*0.47).round()as u16;for x in 1..c.width.saturating_sub(1){text(b,c,x,board_y,"─",t.wire);}text(b,c,c.width/2-5,board_y," LOGIC BOARD ",t.muted);
    let heat=s.cpu_temp.value().copied().unwrap_or(30.).max(s.gpu_temp.value().copied().unwrap_or(30.));let heat_color=t.heat(heat);
    for n in [NRect::new(0.12,0.02,0.20,0.07),NRect::new(0.68,0.02,0.20,0.07)]{let r=to_cells(n,c,mirror);for y in 0..r.height{line(b,r,y,"║".repeat(r.width as usize),heat_color);}}
    let pipe=to_cells(NRect::new(0.22,0.07,0.56,0.04),c,mirror);line(b,pipe,0,"═".repeat(pipe.width as usize),heat_color);a.parts.push((Part::Thermal,pipe));a.hitmap.add(pipe,4,Hit::Part(Part::Thermal));
    for spec in COMPONENTS{let p=spec.part;let r=to_cells(spec.rect,c,mirror);let selected=a.selection==Some(p);let activity=crate::app::metric(p,&s);let color=if selected{t.accent}else if a.cfg.teardown.highlight_active&&activity>0.02{board::tint(p,&t)}else{t.wire};let label=if a.cfg.teardown.labels{spec.label}else{""};let inside=outline(b,r,label,spec.provenance,color,&t);a.parts.push((p,r));a.hitmap.add(r,3,Hit::Part(p));
        match p{
            Part::Fan0|Part::Fan1=>{if let Some(fan)=s.fans.value().and_then(|f|f.get(if p==Part::Fan0{0}else{1})){let rotor_area=Rect::new(inside.x,inside.y,inside.width,inside.height.saturating_sub(1));let phase=if a.motion==crate::motion::Motion::Full{a.anim.phases.get(&fan.name).copied().unwrap_or(0.)}else{0.};rotor(b,rotor_area,phase,if fan.rpm>0{t.accent}else{t.wire},ascii);line(b,inside,inside.height.saturating_sub(1),format!("{} rpm{}",fan.rpm,if large{" · side assumed"}else{""}),t.text);}else{line(b,inside,0,s.fans.label(),t.muted);}},
            Part::Soc=>{line(b,inside,0,"M4 PRO",t.cpu);line(b,inside,1,format!("C {:3.0}%",s.cpu*100.),t.text);if let Some(g)=s.gpu.value(){line(b,inside,2,format!("G {:3.0}%",g*100.),t.gpu);}line(b,inside,3,temp(&s.cpu_temp,&a.cfg.units),heat_color);},
            Part::Memory=>{let v=s.memory_ratio();for y in 0..inside.height.saturating_sub(1){meter(b,row(inside,y,1),v,t.memory,&t,ascii);}line(b,inside,inside.height.saturating_sub(1),format!("{:.1}G",s.memory_used as f64/1024f64.powi(3)),t.text);},
            Part::Power=>line(b,inside,0,watts(&s.total_power()),t.accent),Part::Disk=>line(b,inside,0,s.disk.value().map_or("N/A".into(),|f|format!("R {:.0}M",f.read/1e6)),t.disk),
            Part::Battery=>{line(b,inside,1,s.battery.value().map_or("N/A".into(),|v|format!("{:.0}%",v.percent)),t.battery);for y in 3..inside.height{line(b,inside,y,if s.battery.value().is_some_and(|v|v.charging){"‹"}else{"›"},t.battery);}},
            Part::Network=>line(b,inside,0,s.network.value().map_or("N/A".into(),|f|format!("↓ {}",rate(f.read,&a.cfg.units,true))),t.net),_=>{}
        }
        if selected{reticle(b,r,t.bright);}
    }
    for(i,n)in CELLS.iter().enumerate(){let r=to_cells(*n,c,mirror);let selected=a.selection==Some(Part::Cell(i as u8));if a.cells{let p=a.resolve(Part::Cell(i as u8));let inside=outline(b,r,&format!("{} / {}",i+1,p.name().to_uppercase()),Provenance::Photo,if selected{t.accent}else{board::tint(p,&t)},&t);line(b,inside,0,board::readout(p,&s,a),t.text);let hist=a.history_values(p);let max=hist.iter().copied().fold(1.,f32::max);if inside.height>=3{spark(b,Rect::new(inside.x,inside.y+1,inside.width,inside.height-2),&hist,max,board::tint(p,&t),&t,ascii);}if inside.height>=2{line(b,inside,inside.height-1,match p{Part::Cpu=>format!("{} tiers · {} cores",s.tiers.len(),s.core_count()),Part::Memory=>format!("PRESSURE {}",s.pressure.value().map_or("N/A",String::as_str)),Part::Power=>format!("SYS {}",watts(&s.system_power)),_=>"60s history".into()},t.muted);}a.parts.push((Part::Cell(i as u8),r));a.hitmap.add(r,3,Hit::Part(Part::Cell(i as u8)));}else{let inside=outline(b,r,if a.cfg.teardown.labels{["CELL 1","CELL 2","CELL 3","CELL 4","CELL 5","CELL 6"][i]}else{""},Provenance::Photo,t.wire,&t);line(b,inside,0,"▄▄",t.muted);if inside.height>3{line(b,inside,inside.height/2,"BATTERY",t.wire);}a.hitmap.add(r,2,Hit::Part(Part::Battery));}if selected{reticle(b,r,t.bright);}}
    for(n,label)in[(NRect::new(0.005,0.56,0.045,0.32),"SPK"),(NRect::new(0.95,0.56,0.045,0.32),"SPK")]{let r=to_cells(n,c,mirror);for y in 0..r.height{line(b,r,y,"░".repeat(r.width as usize),t.wire);}line(b,r,r.height/2,label,t.muted);}
    let recess=to_cells(NRect::new(0.30,0.86,0.40,0.10),c,mirror);outline(b,recess,"TRACKPAD RECESS",Provenance::Approximate,t.wire,&t);
    for(left,labels)in[(true,["HDMI","SDXC","TB5"]),(false,["MAG","TB5","3.5"]) ]{for(i,label)in labels.iter().enumerate(){let x=if left!=mirror{1}else{c.width.saturating_sub(5)};text(b,c,x,(c.height as f32*(0.17+i as f32*0.10))as u16,label,t.muted);}}
}
fn generic(b:&mut Buffer,r:Rect,a:&mut App){let t=a.theme.clone();let s=a.sample();let parts:Vec<_>=[Part::Soc,Part::Memory,Part::Power,Part::Disk,Part::Network,Part::Thermal].into_iter().chain(if s.battery.value().is_some(){Some(Part::Battery)}else{None}).collect();let cols=3;let rows=(parts.len()as u16).div_ceil(cols);let w=r.width/cols;let h=r.height/rows;for(i,p)in parts.into_iter().enumerate(){board::panel(b,Rect::new(r.x+i as u16%cols*w,r.y+i as u16/cols*h,w.saturating_sub(1),h.saturating_sub(1)),p,a);}line(b,r,r.height.saturating_sub(1),"Telemetry modules · schematic arrangement",t.muted);}
fn navigator(b:&mut Buffer,r:Rect,a:&mut App){let t=a.theme.clone();let s=a.sample();let left=r.width*45/100;let order=a.part_order();let rows=r.height.saturating_sub(2)as usize;line(b,r,0,"COMPONENT NAVIGATOR",t.accent);for(i,p)in order.iter().take(rows).enumerate(){let selected=a.selection==Some(*p);let rr=Rect::new(r.x,r.y+2+i as u16,left,1);line(b,rr,0,format!("{} {}",if selected{"›"}else{" "},p.name()),if selected{t.accent}else{t.muted});a.parts.push((*p,rr));a.hitmap.add(rr,2,Hit::Part(*p));}let p=a.resolve(a.selection.unwrap_or(Part::Soc));let right=Rect::new(r.x+left+1,r.y+1,r.width-left-1,r.height.saturating_sub(1));let inside=frame(b,right,&p.name().to_uppercase(),"",true,&t);line(b,inside,0,board::readout(p,&s,a),t.text);if inside.height>3{let inner=Rect::new(inside.x,inside.y+2,inside.width,inside.height-2);board::content(b,inner,p,&s,a);}}
fn selected(b:&mut Buffer,r:Rect,a:&App){let t=&a.theme;let p=a.resolve(a.selection.unwrap_or(Part::Soc));let s=a.sample();let card=Rect::new(r.x,r.y,r.width,(r.height*55/100).max(8));let inner=frame(b,card,"SELECTED","Enter / z",true,t);line(b,inner,1,p.name().to_uppercase(),board::tint(p,t));line(b,inner,3,board::readout(p,&s,a),t.text);let h=(inner.height/4).max(1);let values=a.history_values(p);spark(b,Rect::new(inner.x,inner.y+5,inner.width,h.min(inner.height.saturating_sub(5))),&values,values.iter().copied().fold(1.,f32::max),board::tint(p,t),t,a.cfg.ui.glyphs=="ascii");let text_rect=Rect::new(inner.x,inner.y+(6+h).min(inner.height),inner.width,inner.height.saturating_sub(6+h));ratatui::widgets::Paragraph::new(p.note()).wrap(ratatui::widgets::Wrap{trim:true}).style(Style::new().fg(t.muted)).render(text_rect,b);
    let legend=Rect::new(r.x,card.bottom(),r.width,r.height-card.height);let inner=frame(b,legend,"READING THE MACHINE","",false,t);for(i,line0)in["──  Photo-matched outline","┄┄  Approximate position","┈┈  Conceptual overlay","","Rotor speed follows RPM","Pipe color follows temperature","Trace density follows power","","m   toggle telemetry cells","o   mirror orientation","L   toggle labels","H   highlight activity"].iter().enumerate(){line(b,inner,i as u16,line0,t.muted);}}
fn reticle(b:&mut Buffer,r:Rect,c:ratatui::style::Color){if r.width<2||r.height<2{return;}for(x,y,s)in[(0,0,"┏"),(r.width-1,0,"┓"),(0,r.height-1,"┗"),(r.width-1,r.height-1,"┛")]{text(b,r,x,y,s,c);}}
