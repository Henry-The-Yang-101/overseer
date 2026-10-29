use ratatui::{buffer::Buffer,layout::{Rect,Position},style::{Color,Style},text::Line,widgets::{Block,BorderType,Widget}};
use unicode_width::UnicodeWidthStr;
use crate::{metrics::Avail,config::Units};
use super::theme::{Theme,blend};
pub fn text(b:&mut Buffer,r:Rect,x:u16,y:u16,s:impl AsRef<str>,c:Color){if x<r.width&&y<r.height{b.set_stringn(r.x+x,r.y+y,s.as_ref(),(r.width-x)as usize,Style::default().fg(c));}}
pub fn line(b:&mut Buffer,r:Rect,y:u16,s:impl AsRef<str>,c:Color){text(b,r,0,y,s,c);}
pub fn inner(r:Rect,x:u16,y:u16)->Rect{Rect::new(r.x+x.min(r.width),r.y+y.min(r.height),r.width.saturating_sub(2*x),r.height.saturating_sub(2*y))}
pub fn row(r:Rect,y:u16,h:u16)->Rect{Rect::new(r.x,r.y+y.min(r.height),r.width,h.min(r.height.saturating_sub(y)))}
pub fn frame(b:&mut Buffer,r:Rect,title:&str,value:&str,focused:bool,t:&Theme)->Rect {
    if r.width<2||r.height<2{return r;}
    let mut block=Block::bordered().border_type(if focused{BorderType::Thick}else{BorderType::Rounded}).border_style(Style::new().fg(if focused{t.accent}else{t.wire})).style(Style::new().bg(t.panel)).title_top(Line::from(format!(" {title} ")).style(Style::new().fg(if focused{t.accent}else{t.muted}).bold()));
    if !value.is_empty()&&r.width as usize>title.width()+value.width()+7{block=block.title_top(Line::from(format!(" {value} ")).right_aligned().style(Style::new().fg(t.text)));}
    let a=block.inner(r);block.render(r,b);inner(a,1,0)
}
pub fn truncate(s:&str,w:usize)->String{if s.width()<=w{return s.into();}if w==0{return String::new();}let mut out=String::new();for c in s.chars(){if out.width()+unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)>w-1{break;}out.push(c);}out.push('…');out}
pub fn meter(b:&mut Buffer,r:Rect,v:f32,c:Color,t:&Theme,ascii:bool){if r.width==0||r.height==0{return;}let v=if v.is_finite(){v.clamp(0.,1.)}else{0.};let n=v*r.width as f32;for x in 0..r.width{let f=(n-x as f32).clamp(0.,1.);let glyph=if ascii{if f>0.5{'#'}else{'.'}}else if f>=1.{'█'}else if f>0.{[' ','▏','▎','▍','▌','▋','▊','▉'][(f*8.)as usize]}else{'─'};text(b,r,x,0,glyph.to_string(),if f>0.{c}else{t.substrate});}}
pub fn avail<T>(v:&Avail<T>,format:impl Fn(&T)->String)->String{match v{Avail::Ok(v)=>format(v),Avail::Estimated(v)=>format!("≈{}",format(v)),Avail::Stale{value,age_s}=>format!("{} · {age_s}s old",format(value)),_=>v.label().into()}}
pub fn watts(v:&Avail<f32>)->String{avail(v,|v|format!("{v:5.2} W"))}
pub fn percent(v:f32)->String{format!("{:5.1}%",if v.is_finite(){(v*100.).clamp(0.,100.)}else{0.})}
pub fn freq(mhz:u32)->String{if mhz==0{"     N/A".into()}else if mhz>=1000{format!("{:4.2} GHz",mhz as f32/1000.)}else{format!("{mhz:4} MHz")}}
pub fn bytes(v:u64,u:&Units)->String{let base=if u.bytes=="si"{1000.}else{1024.};let mut n=v as f64;let mut i=0;while n>=base&&i<4{n/=base;i+=1;}let unit=if u.bytes=="si"{["B","KB","MB","GB","TB"][i]}else{["B","KiB","MiB","GiB","TiB"][i]};format!("{n:.1} {unit}")}
pub fn rate(v:f64,u:&Units,network:bool)->String{if network&&u.net=="bits"{let mut n=v*8.;let mut i=0;while n>=1000.&&i<3{n/=1000.;i+=1;}format!("{n:.1} {}/s",["b","Kb","Mb","Gb"][i])}else{format!("{}/s",bytes(v.max(0.)as u64,u))}}
pub fn temp(v:&Avail<f32>,u:&Units)->String{avail(v,|v|if u.temp=="f"{format!("{:.0}°F",v*1.8+32.)}else{format!("{v:.0}°C")})}
pub fn duration(s:f64)->String{let n=s.max(0.)as u64;if n>=3600{format!("{}:{:02}:{:02}",n/3600,n/60%60,n%60)}else{format!("{:02}:{:02}",n/60,n%60)}}
pub fn unavailable<T>(b:&mut Buffer,r:Rect,v:&Avail<T>,t:&Theme){for y in 0..r.height{for x in 0..r.width{if (x+y)%4==0{text(b,r,x,y,"╱",t.substrate);}}}line(b,r,r.height/2,v.label(),t.muted);if r.height>2{line(b,r,(r.height/2+1).min(r.height-1),v.reason(),t.muted);}}
#[derive(Clone)]
pub struct BrailleGrid{w:u16,h:u16,bits:Vec<u8>,color:Vec<Color>}
impl BrailleGrid{
    pub fn new(w:u16,h:u16)->Self{Self{w,h,bits:vec![0;w as usize*h as usize],color:vec![Color::Reset;w as usize*h as usize]}}
    pub fn set(&mut self,x:i32,y:i32,c:Color){if x<0||y<0||x>=self.w as i32*2||y>=self.h as i32*4{return;}let i=y as usize/4*self.w as usize+x as usize/2;self.bits[i]|=[[1,8],[2,16],[4,32],[64,128]][y as usize%4][x as usize%2];self.color[i]=c;}
    pub fn line(&mut self,a:(f32,f32),z:(f32,f32),c:Color){let n=((z.0-a.0).abs().max((z.1-a.1).abs()).ceil()as usize).max(1);for i in 0..=n{let t=i as f32/n as f32;self.set((a.0+(z.0-a.0)*t).round()as i32,(a.1+(z.1-a.1)*t).round()as i32,c);}}
    pub fn circle(&mut self,x:f32,y:f32,r:f32,c:Color){let n=(r*8.).ceil()as usize;for i in 0..=n{let a=i as f32/n.max(1)as f32*std::f32::consts::TAU;self.set((x+r*a.cos()).round()as i32,(y+r*a.sin()).round()as i32,c);}}
    pub fn flush(&self,r:Rect,b:&mut Buffer,ascii:bool){for(i,&bits)in self.bits.iter().enumerate(){if bits!=0 {let x=i%self.w as usize;let y=i/self.w as usize;if x>=r.width as usize||y>=r.height as usize{continue;}if let Some(cell)=b.cell_mut((r.x+x as u16,r.y+y as u16)){cell.set_char(if ascii{if bits.count_ones()>4{':'}else{'.'}}else{char::from_u32(0x2800+bits as u32).unwrap()}).set_fg(self.color[i]);}}}}
}
pub fn max_pool(data:&[f32],n:usize)->Vec<f32>{if n==0{return vec![];}if data.len()<=n{return data.to_vec();}(0..n).map(|i|data[i*data.len()/n..((i+1)*data.len()/n).max(i*data.len()/n+1)].iter().copied().fold(0.,f32::max)).collect()}
pub fn spark(b:&mut Buffer,r:Rect,data:&[f32],max:f32,c:Color,t:&Theme,ascii:bool){if r.is_empty(){return;}let data=max_pool(data,r.width as usize*2);let mut g=BrailleGrid::new(r.width,r.height);let offset=r.width as usize*2-data.len();let h=r.height as i32*4;for(i,v)in data.iter().enumerate(){let count=((*v/max.max(0.001)).clamp(0.,1.)*h as f32).round()as i32;for y in 0..count.max(1){g.set((i+offset)as i32,h-1-y,blend(t.wire,c,0.35+0.65*y as f32/h.max(1)as f32));}}g.flush(r,b,ascii);}
pub fn rotor(b:&mut Buffer,r:Rect,phase:f32,c:Color,ascii:bool){if r.width<3||r.height<2{return;}let mut g=BrailleGrid::new(r.width,r.height);let(cx,cy)=(r.width as f32-0.5,r.height as f32*2.-0.5);let rad=(cx.min(cy)-1.).max(1.);g.circle(cx,cy,rad,c);g.circle(cx,cy,rad*0.24,c);for i in 0..9{let a=phase*std::f32::consts::TAU+i as f32*std::f32::consts::TAU/9.;let mut old=None;for j in 0..=8{let f=j as f32/8.;let d=rad*(0.3+0.57*f);let p=(cx+d*(a-0.55*f).cos(),cy+d*(a-0.55*f).sin());if let Some(o)=old{g.line(o,p,c);}old=Some(p);}}g.flush(r,b,ascii);}
pub fn arc(b:&mut Buffer,r:Rect,value:f32,c:Color,t:&Theme,ascii:bool){let mut g=BrailleGrid::new(r.width,r.height);let(cx,cy)=(r.width as f32-0.5,r.height as f32*2.-0.5);let rad=(cx.min(cy)-1.).max(1.);for i in 0..200{let f=i as f32/199.;let a=(0.75+f*1.5)*std::f32::consts::PI;for dr in [0.,1.]{g.set((cx+(rad-dr)*a.cos())as i32,(cy+(rad-dr)*a.sin())as i32,if f<=value{c}else{t.wire});}}g.flush(r,b,ascii);}
pub fn digits(b:&mut Buffer,r:Rect,value:u32,c:Color){const D:[[&str;3];10]=[["┏━┓","┃ ┃","┗━┛"],["╺┓ "," ┃ ","╺┻╸"],["╺━┓","┏━┛","┗━╸"],["╺━┓"," ━┫","╺━┛"],["╻ ╻","┗━┫","  ╹"],["┏━╸","┗━┓","╺━┛"],["┏━╸","┣━┓","┗━┛"],["╺━┓","  ┃","  ╹"],["┏━┓","┣━┫","┗━┛"],["┏━┓","┗━┫","╺━┛"]];for(i,c0)in value.to_string().chars().enumerate(){let n=c0.to_digit(10).unwrap()as usize;for y in 0..3{text(b,r,i as u16*4,y,D[n][y as usize],c);}}}
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum Hit {Part(crate::app::Part),Process((u32,u64)),Screen(crate::app::Screen),Rail,Command(usize)}
#[derive(Default,Clone)]pub struct HitMap(pub Vec<(Rect,u8,Hit)>);
impl HitMap{pub fn add(&mut self,r:Rect,z:u8,h:Hit){self.0.push((r,z,h));}pub fn at(&self,p:Position)->Option<&Hit>{self.0.iter().enumerate().filter(|(_, (r,_,_))|r.contains(p)).max_by_key(|(i,(_,z,_))|(*z,*i)).map(|(_,(_,_,h))|h)}}
