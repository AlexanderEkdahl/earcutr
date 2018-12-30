#![allow(dead_code)]

use std::sync::mpsc::sync_channel;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::SyncSender;
use std::thread;

static NULL: usize = std::usize::MAX;
static NULL32: u32 = std::u32::MAX;
static DEBUG: usize = 0;
type NodeIdx = usize;

/*
Note: this program implements a linked list on top of a Rust Vector.
See README.md for more information.
*/

impl Node {
    fn new(i: usize, x: f64, y: f64, idx: usize) -> Node {
        Node {
            i: i,
            x: x,
            y: y,
            prev_idx: NULL,
            next_idx: NULL,
            z: NULL32,
            nextz_idx: NULL,
            prevz_idx: NULL,
            steiner: false,
            idx: idx,
        }
    }
}

#[derive(Clone)]
struct Node {
    i: usize,         // vertex index into the main 1-dimension array of f64s
    x: f64,           // vertex x coordinate
    y: f64,           // vertex y coordinate
    prev_idx: usize,  // previous vertex nodes in a polygon ring
    next_idx: usize,  // next vertex node in the polygon ring
    z: u32,           // z-order curve value
    prevz_idx: usize, // previous  nodes in z-order
    nextz_idx: usize, // next node in the polygon ring, in z-order
    steiner: bool,    // indicates whether this is a steiner point
    idx: usize,       // index of this node in LinkedLists.nodes
}

macro_rules! dlog {
	($loglevel:expr, $($s:expr),*) => (
		if DEBUG>=$loglevel { print!("{}:",$loglevel); println!($($s),+); }
	)
}

// Note: only 'node!' works for Left-Hand-Side of assignment.
macro_rules! node {
    ($ll:expr,$idx:expr) => {
        $ll.nodes[$idx]
    };
}
macro_rules! next {
    ($ll:ident,$idx:expr) => {
        $ll.nodes[$ll.nodes[$idx].next_idx]
    };
}
macro_rules! prev {
    ($ll:ident,$idx:expr) => {
        $ll.nodes[$ll.nodes[$idx].prev_idx]
    };
}
macro_rules! prevz {
    ($ll:ident,$idx:expr) => {
        $ll.nodes[$ll.nodes[$idx].prevz_idx]
    };
}

struct BoundingBox {
	minx: f64,
	miny: f64,
	maxx: f64,
	maxy: f64
}
impl BoundingBox {
	fn expand(mut self,n:&Node)->BoundingBox{
		self.maxx = f64::max(self.maxx,n.x);
		self.maxy = f64::max(self.maxy,n.y);
		self.minx = f64::min(self.minx,n.x);
		self.miny = f64::min(self.miny,n.y);
		self
	}
	fn new(ll:&LinkedLists,r: std::ops::Range<NodeIdx>)->BoundingBox {
		let p = &node!(ll,r.start);
		let bb = BoundingBox{maxx:p.x,maxy:p.y,minx:p.x,miny:p.y};
		ll.iter_range(r).fold(bb, |b,ref mut n| b.expand( &n ))
	}
}

struct LinkedLists {
    nodes: Vec<Node>,
    freelist: Vec<NodeIdx>, // removed nodes have their index stored here
	snd: std::sync::mpsc::SyncSender<u32>,
	rcv: std::sync::mpsc::Receiver<u32>, 
}

// https://www.cs.hmc.edu/~geoff/classes/hmc.cs070.200101/homework10/hashfuncs.html
// https://stackoverflow.com/questions/1908492/unsigned-integer-in-javascript
fn horsh(mut h: u32, n: u32) -> u32 {
    let highorder = h & 0xf8000000;
    h = h << 5;
    h = h ^ (highorder >> 27);
    h = h ^ n;
    return h;
}

fn pn(a: usize) -> String {
    match a {
        std::usize::MAX => String::from("NULL"),
        _ => a.to_string(),
    }
}
fn pz(a: u32) -> String {
    match a {
        std::u32::MAX => String::from("NULL"),
        _ => a.to_string(),
    }
}
fn pb(a: bool) -> String {
    match a {
        true => String::from("x"),
        false => String::from(" "),
    }
}

impl LinkedLists {
    fn iter(&self, startidx: NodeIdx) -> NodeIterator {
        return NodeIterator::new(self, startidx, startidx);
    }
    fn iter_range(&self, r: std::ops::Range<NodeIdx>) -> NodeIterator {
        return NodeIterator::new(self, r.start, r.end);
    }
    fn insert_node(&mut self, i: usize, x: f64, y: f64, last: NodeIdx) -> NodeIdx {
        let mut p = Node::new(i, x, y, self.nodes.len());
        if last == NULL {
            p.next_idx = p.idx;
            p.prev_idx = p.idx;
        } else {
            p.next_idx = node!(self, last).next_idx;
            p.prev_idx = last;
            let lastnextidx = node!(self, last).next_idx;
            node!(self, lastnextidx).prev_idx = p.idx;
            node!(self, last).next_idx = p.idx;
        }
        self.nodes.push(p.clone());
        return p.idx;
    }
    fn dump(&self) -> String {
        let mut s = format!("ll, #nodes: {}", self.nodes.len());
        s.push_str(&format!(
            " #used: {}\n",
            self.nodes.len() - self.freelist.len()
        ));
        s.push_str(&format!(
            " {:>3} {:>3} {:>4} {:>4} {:>8.3} {:>8.3} {:>4} {:>4} {:>4} {:>2} {:>2} {:>2}\n",
            "vi", "i", "p", "n", "x", "y", "pz", "nz", "z", "st", "fr", "cyl"
        ));
        for n in self.nodes.iter() {
            s.push_str(&format!(
                " {:>3} {:>3} {:>4} {:>4} {:>8.3} {:>8.3} {:>4} {:>4} {:>4} {:>2} {:>2} {:>2}\n",
                n.idx,
                n.i,
                pn(n.prev_idx),
                pn(n.next_idx),
                n.x,
                n.y,
                pn(n.prevz_idx),
                pn(n.nextz_idx),
                pz(n.z),
                pb(n.steiner),
                pb(self.freelist.contains(&n.idx)),
                self.cycle_len(n.idx),
            ));
        }
        return s;
    }
    fn cycle_len(&self, p: NodeIdx) -> usize {
        if p >= self.nodes.len() {
            return 0;
        }
        let end = node!(self, p).prev_idx;
        let mut i = p;
        let mut count = 1;
        loop {
            i = node!(self, i).next_idx;
            count += 1;
            if i == end {
                break count;
            }
            if count > self.nodes.len() {
                break count;
            }
        }
    }
    fn remove_node(&mut self, p: NodeIdx) {
        dlog!(4, "fn remove_node {}", node!(self, p).i);
        if p == NULL {
            return;
        }

        let nx = node!(self, p).next_idx;
        let pr = node!(self, p).prev_idx;
        node!(self, nx).prev_idx = pr;
        node!(self, pr).next_idx = nx;

        let prz = node!(self, p).prevz_idx;
        let nxz = node!(self, p).nextz_idx;
        if prz != NULL {
            node!(self, prz).nextz_idx = nxz;
        }
        if nxz != NULL {
            node!(self, nxz).prevz_idx = prz;
        }

        if self.freelist.contains(&p) {
            return;
        }
        self.freelist.push(p);
    }
    fn new() -> LinkedLists {
		let (sender, receiver) = sync_channel(1);
        LinkedLists {
            nodes: Vec::new(),
            freelist: Vec::new(),
			snd: sender,
			rcv: receiver,
        }
//fn setz(&mut self
    }
} // ll

fn compare_x(a: &Node, b: &Node) -> std::cmp::Ordering {
    return a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal);
}

fn compare_y(a: &Node, b: &Node) -> std::cmp::Ordering {
    return a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal);
}

// link every hole into the outer loop, producing a single-ring polygon
// without holes
fn eliminate_holes(
    ll: &mut LinkedLists,
    data: &Vec<f64>,
    hole_indices: &Vec<usize>,
    inouter_node: NodeIdx,
    dim: usize,
) -> NodeIdx {
    dlog!(
        4,
        "fn eliminate_holes dlen:{} holeixs:{:?} onodevi:{} onodei:{} dm:{}",
        data.len(),
        hole_indices,
        node!(ll, inouter_node).idx,
        node!(ll, inouter_node).i,
        dim
    );
    dlog!(9, "fn eliminate_holes ll {}", ll.dump());
    let mut outer_node = inouter_node;
    let mut queue: Vec<Node> = Vec::new();
    let hlen = hole_indices.len();
    for i in 0..hlen {
        let start = hole_indices[i] * dim;
        let end = if i < (hlen - 1) {
            hole_indices[i + 1] * dim
        } else {
            data.len()
        };
        let list = linked_list_add_contour(ll, &data, start, end, dim, false);
        if list == node!(ll, list).next_idx {
            ll.nodes[list].steiner = true;
        }
        let leftmost = ll.iter(list).min_by(|n, m| compare_x(n, m)).unwrap();
        queue.push(leftmost.clone());
    }

    queue.sort_by(|n, m| compare_x(n, m));

    // process holes from left to right
    for i in 0..queue.len() {
        eliminate_hole(ll, queue[i].idx, outer_node);
        let nextidx = next!(ll, outer_node).idx;
        outer_node = filter_points(ll, outer_node, nextidx);
    }
    return outer_node;
} // elim holes

// invsize is used to transform coords into integers for z-order calculation
fn calc_invsize(minx: f64, miny: f64, maxx: f64, maxy: f64) -> f64 {
    let (dx, dy) = (maxx - minx, maxy - miny);
    match (dx > dy, dx == 0.0, dy == 0.0) {
        (true, true, _) => 0.0,
        (true, false, _) => 1.0 / dx,
        (false, _, true) => 0.0,
        (false, _, false) => 1.0 / dy,
    }
}

// main ear slicing loop which triangulates a polygon (given as a linked
// list)
fn earcut_linked(
    ll: &mut LinkedLists,
    mut ear: NodeIdx,
    triangles: &mut Vec<usize>,
    dim: usize,
    minx: f64,
    miny: f64,
    invsize: f64,
    pass: usize,
) {
    dlog!(
        4,
        "fn earcut_linked ear.i:{} nodes:{} tris:{} dm:{} mx:{} my:{} invs:{} pas:{}",
        node!(ll, ear).i,
        ll.nodes.len(),
        triangles.len(),
        dim,
        minx,
        miny,
        invsize,
        pass
    );

    if ear == NULL {
        return;
    }

    // interlink polygon nodes in z-order
    // note this does nothing for smaller data len, b/c invsize will be 0
    if pass == 0 && invsize > 0.0 {
        index_curve(ll, ear, minx, miny, invsize);
    }

    let mut stop = ear;
    let mut prev = 0;
    let mut next = 0;
    // iterate through ears, slicing them one by one
    while node!(ll, ear).prev_idx != node!(ll, ear).next_idx {
        dlog!(9, "p{} e{} n{} s{}", prev, ear, next, stop);
        prev = node!(ll, ear).prev_idx;
        next = node!(ll, ear).next_idx;

		if (invsize==0.0 && is_ear(ll,ear)) || (invsize!=0.0 && is_ear_hashed(ll,ear,minx,miny,invsize)) {
            // cut off the triangle
            triangles.push(ll.nodes[prev].i / dim);
            triangles.push(ll.nodes[ear].i / dim);
            triangles.push(ll.nodes[next].i / dim);

            ll.remove_node(ear);

            // skipping the next vertex leads to less sliver triangles
            ear = ll.nodes[next].next_idx;
            stop = ll.nodes[next].next_idx;
            continue;
        }

        ear = next;

        // if we looped through the whole remaining polygon and can't
        // find any more ears
        if ear == stop {
            if pass == 0 {
                // try filtering points and slicing again
                let tmp = filter_points(ll, ear, NULL);
                earcut_linked(ll, tmp, triangles, dim, minx, miny, invsize, 1);
            } else if pass == 1 {
                // if this didn't work, try curing all small
                // self-intersections locally
                ear = cure_local_intersections(ll, ear, triangles, dim);
                earcut_linked(ll, ear, triangles, dim, minx, miny, invsize, 2);
            } else if pass == 2 {
                // as a last resort, try splitting the remaining polygon
                // into two
                split_earcut(ll, ear, triangles, dim, minx, miny, invsize);
            }
            break;
        }
    } // while
} //cut_linked

// interlink polygon nodes in z-order
fn index_curve(ll: &mut LinkedLists, start: NodeIdx, minx: f64, miny: f64, invsize: f64) {
	let mut pi = start;
	loop {
        if node!(ll,pi).z == NULL32 {
            node!(ll,pi).z = zorder(node!(ll,pi).x, node!(ll,pi).y, minx, miny, invsize);
        }
        node!(ll,pi).prevz_idx = node!(ll,pi).prev_idx;
		node!(ll,pi).nextz_idx = node!(ll,pi).next_idx;
		pi = match pi {
			start=>break,
			_=>node!(ll,pi).next_idx,
		};
    }

    let pzi = node!(ll, start).prevz_idx;	
    node!(ll, pzi).nextz_idx = NULL;
    node!(ll, start).prevz_idx = NULL;

    sort_linked(ll, start);
}

// Simon Tatham's linked list merge sort algorithm
// http://www.chiark.greenend.org.uk/~sgtatham/algorithms/listsort.html
fn sort_linked(ll: &mut LinkedLists, inlist: NodeIdx) {
    dlog!(4, "fn sort linked begin {}", node!(ll, inlist).i);
    dlog!(5, "{}", ll.dump());
    let mut p;
    let mut q;
    let mut e;
    let mut nummerges;
    let mut psize;
    let mut qsize;
    let mut insize = 1;
    let mut list = inlist;
    let mut tail;

    loop {
        p = list;
        list = NULL;
        tail = NULL;
        nummerges = 0;

        while p != NULL {
            nummerges += 1;
            q = p;
            psize = 0;
            for _ in 0..insize {
                psize += 1;
                q = node!(ll, q).nextz_idx;
                if q == NULL {
                    break;
                }
            }
            qsize = insize;

            while psize > 0 || (qsize > 0 && q != NULL) {
                if psize != 0 && (qsize == 0 || q == NULL || node!(ll, p).z <= node!(ll, q).z) {
                    e = p;
                    p = ll.nodes[p].nextz_idx;
                    psize -= 1;
                } else {
                    e = q;
                    q = ll.nodes[q].nextz_idx;
                    qsize -= 1;
                }

                if tail != NULL {
                    ll.nodes[tail].nextz_idx = e;
                } else {
                    list = e;
                }

                ll.nodes[e].prevz_idx = tail;
                tail = e;
            }

            p = q;
        }

        ll.nodes[tail].nextz_idx = NULL;
        insize *= 2;
        if nummerges <= 1 {
            break;
        }
    } // while (nummerges > 1);
    dlog!(4, "sort linked end");
    dlog!(5, "{}", ll.dump());
} // end sort

// check whether a polygon node forms a valid ear with adjacent nodes
fn is_ear(ll: &LinkedLists, ear: usize) -> bool {
    let (a, b, c) = (&prev!(ll, ear), &node!(ll, ear), &next!(ll, ear));
    if area_local(ll,ear) >= 0.0 {
        dlog!(8, " reflex, can't be an ear");
        return false;
    }
    dlog!(8, "make sure there's not any points inside potential ear");
    !ll.iter_range(c.next_idx..a.idx).any(|n| {
        point_in_triangle(a, b, c, n) && (area_local(ll,n.idx)>=0.0)
//area(&prev!(ll, n.idx), n, &next!(ll, n.idx)) >= 0.0)
    })
}

fn is_ear_hashed(ll: &mut LinkedLists, ear: usize, minx: f64, miny: f64, invsize: f64) -> bool {
    dlog!(
        4,
        "fn is_ear_hashed ear.i:{} minx{} miny{} invs{}",
        node!(ll, ear).i,
        minx,
        miny,
        invsize
    );
    let a = &prev!(ll, ear);
    let b = &node!(ll, ear);
    let c = &next!(ll, ear);
//    if area(&a, &b, &c) >= 0.0 {
    if area_local(ll, ear) >= 0.0 {
        dlog!(9, "reflex, can't be an ear");
        return false;
    }

	let bbox = BoundingBox::new(ll,a.idx..c.idx);

    // z-order range for the current triangle bbox;
    let min_z = zorder(bbox.minx, bbox.miny, minx, miny, invsize);
    let max_z = zorder(bbox.maxx, bbox.maxy, minx, miny, invsize);

    let mut p = node!(ll, ear).prevz_idx;
    let mut n = node!(ll, ear).nextz_idx;

	fn conditions(ll:&LinkedLists,ear:usize, p:usize)->bool {
	    let (a,b,c) = (&prev!(ll, ear),&node!(ll, ear),&next!(ll, ear));
        (p != a.idx)
        && (p != c.idx)
        && point_in_triangle(&a, &b, &c, &node!(ll, p))
        && area(&a, &b, &c) >= 0.0
	}

    dlog!(8, "look for points inside the triangle in both directions");
    while (p != NULL) && 
		(node!(ll, p).z >= min_z) 
		&& (n != NULL) 
		&& (node!(ll, n).z <= max_z) {
		if conditions(ll,ear,p) { return false; }
        p = node!(ll, p).prevz_idx;
		if conditions(ll,ear,n) { return false; }
        n = node!(ll, n).nextz_idx;
    }

    dlog!(8, "look for remaining points in decreasing z-order");
    while (p != NULL) && (node!(ll, p).z >= min_z) {
		if conditions(ll,ear,p) { return false; }
	     p = node!(ll, p).prevz_idx;
    }
    dlog!(8, "look for remaining points in increasing z-order");
    while n != NULL && node!(ll, n).z <= max_z {
		if conditions(ll,ear,n) { return false; }
        n = node!(ll, n).nextz_idx;
    }
    true
}

fn filter_points(ll: &mut LinkedLists, start: NodeIdx, mut end: NodeIdx) -> NodeIdx {
    // eliminate colinear or duplicate points
    if start == NULL {
        dlog!(4, "fn filter points, start null");
        return start;
    }
    if end == NULL {
        end = start;
    }
    if end >= ll.nodes.len() || start >= ll.nodes.len() {
        dlog!(4, "filter problem, {} {} {}", start, end, ll.nodes.len());
        return NULL;
    }
    dlog!(
        4,
        "fn filter points {} {}",
        node!(ll, start).i,
        node!(ll, end).i
    );

    let mut p = start;
    let mut again;
    loop {
        again = false;
        if (!(node!(ll, p).steiner))
            && (equals(&node!(ll, p), &next!(ll, p))
//                || area(&prev!(ll, p), &node!(ll, p), &next!(ll, p)) == 0.0)
                || area_local(ll,p)==0.0)
        {
            ll.remove_node(p);
            end = node!(ll, p).prev_idx;
            p = end;
            if p == node!(ll, p).next_idx {
                break;
            }
            again = true;
        } else {
            p = node!(ll, p).next_idx;
        }
        if !again && p == end {
            break;
        }
    }

    dlog!(4, "fn filter points end {}", node!(ll, end).i);
    return end;
}

// create a circular doubly linked list from polygon points in the
// specified winding order
fn linked_list(
    data: &Vec<f64>,
    start: usize,
    end: usize,
    dim: usize,
    clockwise: bool,
) -> (LinkedLists, usize) {
    let mut ll: LinkedLists = LinkedLists::new();
    let lastidx = linked_list_add_contour(&mut ll, data, start, end, dim, clockwise);
    (ll, lastidx)
}

fn linked_list_add_contour(
    ll: &mut LinkedLists,
    data: &Vec<f64>,
    start: usize,
    end: usize,
    dim: usize,
    clockwise: bool,
) -> usize {
    dlog!(4, "fn linked_list_add_contour");
    if start > data.len() || end > data.len() || dim > end {
        return NULL;
    }
    let mut lastidx = NULL;
    if clockwise == (signed_area(&data, start, end, dim) > 0.0) {
        for i in (start..end).step_by(dim) {
            lastidx = ll.insert_node(i, data[i], data[i + 1], lastidx);
        }
    } else {
        for i in (start..=(end.saturating_sub(dim))).rev().step_by(dim) {
            lastidx = ll.insert_node(i, data[i], data[i + 1], lastidx);
        }
    }

    if equals(&node!(ll, lastidx), &next!(ll, lastidx)) {
        ll.remove_node(lastidx);
        lastidx = node!(ll, lastidx).next_idx;;
    }
    lastidx
}

// z-order of a point given coords and inverse of the longer side of
// data bbox
fn zorder(xf: f64, yf: f64, minx: f64, miny: f64, invsize: f64) -> u32 {
    // coords are transformed into non-negative 15-bit integer range
    let mut x: u32 = 32767 * ((xf - minx) * invsize).round() as u32;
    let mut y: u32 = 32767 * ((yf - miny) * invsize).round() as u32;

    // todo ... big endian?
    x = (x | (x << 8)) & 0x00FF00FF;
    x = (x | (x << 4)) & 0x0F0F0F0F;
    x = (x | (x << 2)) & 0x33333333;
    x = (x | (x << 1)) & 0x55555555;

    y = (y | (y << 8)) & 0x00FF00FF;
    y = (y | (y << 4)) & 0x0F0F0F0F;
    y = (y | (y << 2)) & 0x33333333;
    y = (y | (y << 1)) & 0x55555555;

    x | (y << 1)
}

// check if a point lies within a convex triangle
fn point_in_triangle(a: &Node, b: &Node, c: &Node, p: &Node) -> bool {
    ((c.x - p.x) * (a.y - p.y) - (a.x - p.x) * (c.y - p.y) >= 0.0)
        && ((a.x - p.x) * (b.y - p.y) - (b.x - p.x) * (a.y - p.y) >= 0.0)
        && ((b.x - p.x) * (c.y - p.y) - (c.x - p.x) * (b.y - p.y) >= 0.0)
}

pub fn earcut(data: &Vec<f64>, hole_indices: &Vec<usize>, dim: usize) -> Vec<usize> {
    dlog!(
        4,
        "fn earcut datalen:{} holeis{:?} dim:{}",
        data.len(),
        hole_indices,
        dim
    );
    let mut triangles: Vec<usize> = Vec::new();
    if dim == 0 {
        return triangles;
    };
    let outer_len = match hole_indices.len() {
        0 => data.len(),
        _ => hole_indices[0] * dim,
    };

    let (mut ll, mut outer_node) = linked_list(data, 0, outer_len, dim, true);
    if outer_node == NULL {
        return triangles;
    }
    outer_node = eliminate_holes(&mut ll, data, hole_indices, outer_node, dim);

    let (mut minx, mut miny, mut invsize) = (0.0, 0.0, 0.0);
    // if the shape is not too simple, we'll use z-order curve hash
    // later; calculate polygon bbox
    if data.len() > 80 * dim {
		let bb = BoundingBox::new(&ll,outer_node..outer_node);
		minx = bb.minx;
		miny = bb.miny;
        invsize = calc_invsize(minx, miny, bb.maxx, bb.maxy);
		println!("{} {} {} {} {}",minx,miny,bb.maxx,bb.maxy,invsize);
    }

    // so basically, for data len < 80*dim, minx,miny are 0
    earcut_linked(
        &mut ll,
        outer_node,
        &mut triangles,
        dim,
        minx,
        miny,
        invsize,
        0,
    );
    triangles
}

// signed area of a parallelogram
fn area(p: &Node, q: &Node, r: &Node) -> f64 {
    (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
}

fn area_local(ll:&LinkedLists, qi: NodeIdx) -> f64 {
	area( &prev!(ll,qi), &node!(ll,qi), &next!(ll,qi) )
}

// check if two points are equal
fn equals(p1: &Node, p2: &Node) -> bool {
    p1.x == p2.x && p1.y == p2.y
}

/* go through all polygon nodes and cure small local self-intersections
what is a small local self-intersection? well, lets say you have four points
a,b,c,d. now imagine you have three line segments, a-b, b-c, and c-d. now
imagine two of those segments overlap each other. thats an intersection. so
this will remove one of those nodes so there is no more overlap.

but theres another important aspect of this function. it will dump triangles
into the 'triangles' variable, thus this is part of the triangulation
algorithm itself.*/
fn cure_local_intersections(
    ll: &mut LinkedLists,
    instart: NodeIdx,
    triangles: &mut Vec<NodeIdx>,
    dim: usize,
) -> NodeIdx {
    dlog!(
        4,
        "fn cure_local_intersections i:{},{:?},{}",
        node!(ll, instart).i,
        triangles,
        dim
    );
    let mut p = instart;
    let mut start = instart;
    loop {
        let a = node!(ll, p).prev_idx;
        let b = next!(ll, p).next_idx;

        if !equals(&node!(ll, a), &node!(ll, b))
            && pseudo_intersects(&node!(ll, a), &node!(ll, p), &next!(ll, p), &node!(ll, b))
            && locally_inside(ll, &node!(ll, a), &node!(ll, b))
            && locally_inside(ll, &node!(ll, b), &node!(ll, a))
        {
            triangles.push(node!(ll, a).i / dim);
            triangles.push(node!(ll, p).i / dim);
            triangles.push(node!(ll, b).i / dim);

            // remove two nodes involved
            ll.remove_node(p);
            let nidx = node!(ll, p).next_idx;
            ll.remove_node(nidx);

            start = node!(ll, b).idx;
            p = start;
        }
        p = node!(ll, p).next_idx;
        if p == start {
            break;
        }
    }

    return p;
}

// try splitting polygon into two and triangulate them independently
fn split_earcut(
    ll: &mut LinkedLists,
    start: NodeIdx,
    triangles: &mut Vec<NodeIdx>,
    dim: usize,
    minx: f64,
    miny: f64,
    invsize: f64,
) {
    dlog!(
        4,
        "fn split_earcut i:{} {:?} {} {} {} {}",
        node!(ll, start).i,
        triangles,
        dim,
        minx,
        miny,
        invsize
    );
    // look for a valid diagonal that divides the polygon into two
    let mut a = start;
    loop {
        //	ll.iter( start ).for_each(|an| {
        //		let mut a = an.idx;
        let mut b = next!(ll, a).next_idx;
        while b != node!(ll, a).prev_idx {
            if node!(ll, a).i != node!(ll, b).i
                && is_valid_diagonal(ll, &node!(ll, a), &node!(ll, b))
            {
                // split the polygon in two by the diagonal
                let mut c = split_bridge_polygon(ll, a, b);

                // filter colinear points around the cuts
                let an = node!(ll, a).next_idx;
                let cn = node!(ll, c).next_idx;
                a = filter_points(ll, a, an);
                c = filter_points(ll, c, cn);

                // run earcut on each half
                earcut_linked(ll, a, triangles, dim, minx, miny, invsize, 0);
                earcut_linked(ll, c, triangles, dim, minx, miny, invsize, 0);
                return;
            }
            b = node!(ll, b).next_idx;
        }
        a = node!(ll, a).next_idx;
        if a == start {
            break;
        }
    }
    //);
}

// find a bridge between vertices that connects hole with an outer ring and and link it
fn eliminate_hole(ll: &mut LinkedLists, hole: NodeIdx, outer_node: NodeIdx) {
    dlog!(
        4,
        "fn eliminate_hole hole.i:{} outernode.i:{}",
        node!(ll, hole).i,
        node!(ll, outer_node).i
    );
    let test_node = find_hole_bridge(ll, &node!(ll, hole), outer_node);
    if test_node != NULL {
        let b = split_bridge_polygon(ll, test_node, hole);
        let bn = next!(ll, b).idx;
        filter_points(ll, b, bn);
    }
}

// David Eberly's algorithm for finding a bridge between hole and outer polygon
fn find_hole_bridge(ll: &LinkedLists, hole: &Node, outer_node: NodeIdx) -> NodeIdx {
    dlog!(
        4,
        "fn find_hole_bridge i:{} i:{}",
        hole.i,
        node!(ll, outer_node).i
    );
    let mut qx: f64 = std::f64::NEG_INFINITY;
    let mut m: NodeIdx = NULL;

    // find a segment intersected by a ray from the hole's leftmost
    // point to the left; segment's endpoint with lesser x will be
    // potential connection point

    for p in ll.iter(outer_node)
        .filter(|n| hole.y <= n.y && hole.y >= next!(ll, n.idx).y && next!(ll, n.idx).y != n.y)
		{
                let next = &next!(ll, p.idx);
                let vx = p.x + (hole.y - p.y) * (next.x - p.x) / (next.y - p.y);
                if (vx <= hole.x) && (vx > qx) {
                    qx = vx;
					if vx == hole.x {
                    	if hole.y == p.y {return p.idx}
		                if hole.y == next.y {return next.idx}
        	        }
                    m = match p.x < next.x {
                        true => p.idx,
                        false => next.idx,
                    }
                }
	};

    if m == NULL {
        return NULL;
    }

    // hole touches outer segment; pick lower endpoint
    if hole.x == qx {
        return prev!(ll, m).idx;
    }

    // look for points inside the triangle of hole point, segment
    // intersection and endpoint; if there are no points found, we have
    // a valid connection; otherwise choose the point of the minimum
    // angle with the ray as connection point

    let mut tan_min = std::f64::INFINITY;
    let mut tan = 0.0;
    let mo = node!(ll, m).clone();
    let qhole = &Node::new(0, qx, hole.y, 0);
    let (he, hf) = match hole.y < mo.y {
        true => (hole, qhole),
        false => (qhole, hole),
    };

    ll.iter(mo.next_idx)
        .filter(|n| hole.x > n.x && n.x >= mo.x && point_in_triangle(&he, &mo, &hf, &n))
        .for_each(|p| {
            tan = (hole.y - p.y).abs() / (hole.x - p.x); // tangential

            if (tan < tan_min || (tan == tan_min && p.x > node!(ll, m).x))
                && locally_inside(ll, &p, &hole)
            {
                m = p.idx;
                tan_min = tan;
            }
        });

    return m;
}

// check if a diagonal between two polygon nodes is valid (lies in
// polygon interior)
fn is_valid_diagonal(ll: &LinkedLists, a: &Node, b: &Node) -> bool {
    next!(ll, a.idx).i != b.i
        && prev!(ll, a.idx).i != b.i
        && !intersects_polygon(ll, a, b)
        && locally_inside(ll, a, b)
        && locally_inside(ll, b, a)
        && middle_inside(ll, a, b)
}

/* pseudointersects - check if two segments cross over each other. note
this is different from pure intersction. only two segments crossing over
at some interior point is considered intersection.

line segment p1-q1 vs line segment p2-q2.

Note that if they are collinear, or if the end points touch, or if
one touches the other at one point, it is not considered an intersection.

Please note that the other algorithms in this earcut code depend on this
interpretation of the concept of intersection - if this is modified
so that endpoint touching qualifies as intersection, then it may
affect the larger algorithms and break existing tests.

bsed on https://www.geeksforgeeks.org/check-if-two-given-line-segments-intersect/

*/
fn pseudo_intersects(p1: &Node, q1: &Node, p2: &Node, q2: &Node) -> bool {
    if (equals(p1, p2) && equals(q1, q2)) || (equals(p1, q2) && equals(q1, p2)) {
        return true;
    }
    (area(p1, q1, p2) > 0.0) != (area(p1, q1, q2) > 0.0)
        && (area(p2, q2, p1) > 0.0) != (area(p2, q2, q1) > 0.0)
}

// check if a polygon diagonal intersects any polygon segments
fn intersects_polygon(ll: &LinkedLists, a: &Node, b: &Node) -> bool {
    ll.iter(a.idx).any(|p| {
        p.i != a.i
            && next!(ll, p.idx).i != a.i
            && p.i != b.i
            && next!(ll, p.idx).i != b.i
            && pseudo_intersects(&p, &next!(ll, p.idx), a, b)
    })
}

// check if a polygon diagonal is locally inside the polygon
fn locally_inside(ll: &LinkedLists, a: &Node, b: &Node) -> bool {
    match area_local(ll,a.idx) < 0.0 {
        true => area(a, b, &next!(ll, a.idx)) >= 0.0 && area(a, &prev!(ll, a.idx), b) >= 0.0,
        false => area(a, b, &prev!(ll, a.idx)) < 0.0 || area(a, &next!(ll, a.idx), b) < 0.0,
    }
}

// check if the middle point of a polygon diagonal is inside the polygon
fn middle_inside(ll: &LinkedLists, a: &Node, b: &Node) -> bool {
    let (mx, my) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let mut inside = false;
    ll.iter(a.idx)
        .filter(|p| {
            (p.y > my) != (next!(ll, p.idx).y > my)
                && next!(ll, p.idx).y != p.y
                && (mx < (next!(ll, p.idx).x - p.x) * (my - p.y) / (next!(ll, p.idx).y - p.y) + p.x)
        })
        .for_each(|_| inside = !inside);
    return inside;
}

/* link two polygon vertices with a bridge;

if the vertices belong to the same linked list, this splits the list
into two new lists, representing two new polygons.

if the vertices belong to separate linked lists, it merges them into a
single linked list.

For example imagine one cycle of 6 points, labeled with numbers 0 thru
5, in a single cycle. Now bridge at points 1 and 4. The 2 new polygon
cycles will be like this: 0 1 4 5 0 1 ...  and 1 2 3 4 1 2 3 ....
However because we are using linked lists of nodes, there will be two
new nodes, copies of points 1 and 4. So: the new cycles will be through
nodes 0 1 4 5 0 1 ... and 2 3 6 7 2 3 6 7 .

splitting algorithm:

.0...1...2...3...4...5...     6     7
5p1 0a2 1m3 2n4 3b5 4q0      .c.   .d.

an<-2     an = a.next,
bp<-3     bp = b.prev;
1.n<-4    a.next = b;
4.p<-1    b.prev = a;
6.n<-2    c.next = an;
2.p<-6    an.prev = c;
7.n<-6    d.next = c;
6.p<-7    c.prev = d;
3.n<-7    bp.next = d;
7.p<-3    d.prev = bp;

result of bridge: 2 new cycles.
<0...1> <2...3> <4...5>      <6....7>
5p1 0a4 6m3 2n7 1b5 4q0      7c2  3d6
      x x     x x            x x  x x    // x shows links changed

a b q p a b q p  // begin at a, go next (new cycle 1)
a p q b a p q b  // begin at a, go prev (new cycle 1)
m n d c m n d c  // begin at m, go next (new cycle 2)
m c d n m c d n  // begin at m, go prev (new cycle 2)


Now imagine that we have two cycles, and they are 0 1 2, and 3 4 5.
Bridge at points 1 and 4 will result in a single, long cycle, 0 1 4 5 3 7
6 2 0 1 4 5 ..., where 6 and 1 have the same x,y coordinates, as do 7 and 4.

 0...1...2   3...4...5        6     7
2p1 0a2 1m0 5n4 3b5 4q3      .c.   .d.

an<-2     an = a.next,
bp<-3     bp = b.prev;
1.n<-4    a.next = b;
4.p<-1    b.prev = a;
6.n<-2    c.next = an;
2.p<-6    an.prev = c;
7.n<-6    d.next = c;
6.p<-7    c.prev = d;
3.n<-7    bp.next = d;
7.p<-3    d.prev = bp;

result of bridge: one cycle
 0...1...2   3...4...5        6.....7
2p1 0a4 6m0 5n7 1b5 4q3      7c2   3d6
      x x     x x            x x   x x

a b q n d c m p a b q n d c m .. // begin at a, go next
a p m c d n q b a p m c d n q .. // begin at a, go prev

Return value is the new node, at point 7.
*/
fn split_bridge_polygon(ll: &mut LinkedLists, a: NodeIdx, b: NodeIdx) -> NodeIdx {
    dlog!(
        4,
        "fn split_bridge_polygon a.i:{} b.i:{}",
        node!(ll, a).i,
        node!(ll, b).i
    );
    let cidx = ll.nodes.len();
    let didx = cidx + 1;
    let mut c = Node::new(node!(ll, a).i, node!(ll, a).x, node!(ll, a).y, cidx);
    let mut d = Node::new(node!(ll, b).i, node!(ll, b).x, node!(ll, b).y, didx);

    let an = node!(ll, a).next_idx;
    let bp = node!(ll, b).prev_idx;

    node!(ll, a).next_idx = b;
    node!(ll, b).prev_idx = a;

    c.next_idx = an;
    node!(ll, an).prev_idx = cidx;

    d.next_idx = cidx;
    c.prev_idx = didx;

    node!(ll, bp).next_idx = didx;
    d.prev_idx = bp;

    ll.nodes.push(c);
    ll.nodes.push(d);
    return didx;
}

// return a percentage difference between the polygon area and its
// triangulation area; used to verify correctness of triangulation
pub fn deviation(
    data: &Vec<f64>,
    hole_indices: &Vec<usize>,
    dim: usize,
    triangles: &Vec<usize>,
) -> f64 {
    let outer_len = match hole_indices.len() {
        0 => data.len(),
        _ => hole_indices[0] * dim,
    };

    let mut polygon_area = signed_area(&data, 0, outer_len, dim).abs();
    for i in 0..hole_indices.len() {
        let start = hole_indices[i] * dim;
        let end = match i < (hole_indices.len() - 1) {
            true => hole_indices[i + 1] * dim,
            false => data.len(),
        };
        polygon_area -= signed_area(&data, start, end, dim).abs();
    }

    let mut triangles_area = 0.0;
    for i in (0..triangles.len()).step_by(3) {
        let a = triangles[i] * dim;
        let b = triangles[i + 1] * dim;
        let c = triangles[i + 2] * dim;
        triangles_area += ((data[a] - data[c]) * (data[b + 1] - data[a + 1])
            - (data[a] - data[b]) * (data[c + 1] - data[a + 1]))
            .abs();
    }

    match polygon_area == 0.0 && triangles_area == 0.0 {
        true => 0.0,
        false => ((triangles_area - polygon_area) / polygon_area).abs(),
    }
}

fn signed_area(data: &Vec<f64>, start: usize, end: usize, dim: usize) -> f64 {
    let mut sum = 0.0;
    if dim > end {
        return sum;
    }
    let mut j = end.saturating_sub(dim);
    for i in (start..end).step_by(dim) {
        sum += (data[j] - data[i]) * (data[i + 1] + data[j + 1]);
        j = i;
    }
    sum
}

pub fn flatten(data: &Vec<Vec<Vec<f64>>>) -> (Vec<f64>, Vec<usize>, usize) {
    let mut f64s: Vec<f64> = Vec::new();
    let mut hole_indices: Vec<usize> = Vec::new();
    let dimensions = data[0][0].len();
    let mut hole_index = 0;
    for i in 0..data.len() {
        for j in 0..data[i].len() {
            for d in 0..data[i][j].len() {
                f64s.push(data[i][j][d]);
            }
        }
        if i > 0 {
            hole_index += data[i - 1].len();
            hole_indices.push(hole_index);
        }
    }
    return (f64s, hole_indices, dimensions);
}

//http://www.howtobuildsoftware.com/index.php/how-do/zK2/iterator-rust-how-to-implement-iterator-and-intoiterator-for-a-simple-struct

struct NodeIterator<'a> {
    start: NodeIdx,
    cur: NodeIdx,
    end: NodeIdx,
    count: usize,
    ll: &'a LinkedLists,
}

impl<'a> NodeIterator<'a> {
    fn new(ll: &LinkedLists, start: usize, end: usize) -> NodeIterator {
        NodeIterator {
            start: start,
            cur: start,
            end: end,
            count: 0,
            ll,
        }
    }
}

impl<'a> Iterator for NodeIterator<'a> { 
	type Item = &'a Node; 
	fn  next(&mut self) -> Option<Self::Item> {
        let result = if self.count == 0 {
            Some(&node!(self.ll,self.cur)) // single-node iterator
        } else if self.cur == std::usize::MAX {
            None // NULL node
        } else if self.cur == self.end {
            None // end of iteration
        } else {
            Some(&node!(self.ll, self.cur)) // normal iteration
        };
        self.count += 1;
        self.cur = node!(self.ll, self.cur).next_idx;
        result
    }
}

#[cfg(test)]
impl LinkedLists {
    fn cycles_report(&self) -> String {
        if self.nodes.len() == 0 {
            return format!("[]");
        }
        let mut markv: Vec<usize> = Vec::new();
        markv.resize(self.nodes.len(), NULL);
        let mut cycler;;
        for i in 0..markv.len() {
            if self.freelist.contains(&i) {
                markv[i] = NULL;
            } else if markv[i] == NULL {
                cycler = i;
                let mut p = i;
                let end = node!(self, p).prev_idx;
                markv[p] = cycler;
                let mut count = 0;
                loop {
                    p = node!(self, p).next_idx;
                    markv[p] = cycler;
                    count += 1;
                    if p == end || count > self.nodes.len() {
                        break;
                    }
                } // loop
            } // if markvi == 0
        } //for markv
        format!("cycles report:\n{:?}", markv)
    }

    fn cycle_dump(&self, p: NodeIdx) -> String {
        let mut s = format!("cycle from {}, ", p);
        s.push_str(&format!(" len {}, idxs:", self.cycle_len(p)));
        let mut i = p;
        let end = i;
        loop {
            s.push_str(&format!("{} ", node!(self, i).idx));
            i = node!(self, i).next_idx;
            if i == end {
                break s;
            }
        }
    }

    // find the node with 'i' of starti, horsh it
    fn horsh(&self, starti: usize) -> String {
        let mut s = format!("ll horsh: ");
        let mut startidx: usize = 0;
        for n in self.nodes.iter() {
            if n.i == starti {
                startidx = n.idx;
            };
        }
        let endidx = startidx;
        let mut idx = startidx;
        let mut count = 0;
        let mut state = 0u32;
        loop {
            let n = self.nodes[idx].clone();
            state = horsh(state, n.i as u32);
            idx = next!(self, idx).idx;
            count += 1;
            if idx == endidx || count > self.nodes.len() {
                break;
            }
        }
        s.push_str(&format!(" count:{} horsh: {}", count, state));
        return s;
    }

    fn dump_cycle(&self, start: usize) -> String {
        let mut s = format!("ll, #nodes: {}", self.nodes.len());
        s.push_str(&format!(
            " #used: {}\n",
            self.nodes.len() - self.freelist.len()
        ));
        s.push_str(&format!(
            " {:>3} {:>3} {:>3} {:>4} {:>4} {:>8.3} {:>8.3} {:>4} {:>4} {:>2} {:>2} {:>2}\n",
            "#", "vi", "i", "p", "n", "x", "y", "pz", "nz", "st", "fr", "cyl"
        ));
        let mut startidx: usize = 0;
        for n in self.nodes.iter() {
            if n.i == start {
                startidx = n.idx;
            };
        }
        let endidx = startidx;
        let mut idx = startidx;
        let mut count = 0;
        let mut state = 0u32;
        loop {
            let n = self.nodes[idx].clone();
            state = horsh(state, n.i as u32);
            s.push_str(&format!(
                " {:>3} {:>3} {:>3} {:>4} {:>4} {:>8.3} {:>8.3} {:>4} {:>4} {:>2} {:>2} {:>2}\n",
                count,
                n.idx,
                n.i,
                prev!(self, n.idx).i,
                next!(self, n.idx).i,
                n.x,
                n.y,
                pn(n.prevz_idx),
                pn(n.nextz_idx),
                pb(n.steiner),
                pb(self.freelist.contains(&n.idx)),
                self.cycle_len(n.idx),
            ));
            idx = next!(self, idx).idx;
            count += 1;
            if idx == endidx || count > self.nodes.len() {
                break;
            }
        }
        s.push_str(&format!("dump end, horshcount:{} horsh:{}", count, state));
        return s;
    }
}

mod tests {
    use super::*;
    #[test]
    fn test_iter() {
        let data = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let (mut ll, _) = linked_list(&data, 0, data.len(), 2, true);
        let r = filter_points(&mut ll, 0, 3);
        let leftmost = ll.iter(0).min_by(|n, m| compare_x(n, m)).unwrap();
        assert!(leftmost.x == 0.0);
        assert!(ll.iter(r).any(|n| n.x > 0.5));
        assert!(ll.iter(r).any(|n| n.x < 0.5));
        assert!(ll.iter(r).any(|n| n.idx < 3));
        assert!(!ll.iter(r).any(|n| n.idx > 13));
        println!("{}", ll.dump());
        let mut i = ll.iter(0);
        assert!(i.next().unwrap().idx == 0);
        assert!(i.next().unwrap().idx == 1);
        assert!(i.next().unwrap().idx == 2);
        assert!(i.next().unwrap().idx == 3);
        let blanknode = Node::new(0, 0., 0., 0);
        assert!(equals(&blanknode, i.next().unwrap_or(&blanknode)));
    }

    #[test]
    fn test_linked_list() {
        let dims = 2;
        let data = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let (mut ll, _) = linked_list(&data, 0, data.len(), dims, true);
        assert!(ll.nodes.len() == 4);
        assert!(ll.nodes[0].idx == 0);
        assert!(ll.nodes[0].i == 6);
        assert!(ll.nodes[0].x == 1.0);
        assert!(ll.nodes[0].i == 6 && ll.nodes[0].y == 0.0);
        assert!(ll.nodes[0].next_idx == 1 && ll.nodes[0].prev_idx == 3);
        assert!(ll.nodes[3].next_idx == 0 && ll.nodes[3].prev_idx == 2);
        ll.remove_node(2);
    }

    #[test]
    fn test_point_in_triangle() {
        //        assert!(point_in_triangle(0.0, 0.0, 2.0, 0.0, 2.0, 2.0, 1.0, 0.1));
        //        assert!(!point_in_triangle(0.0, 0.0, 2.0, 0.0, 2.0, 2.0, -1.0, 0.1));
    }

    #[test]
    fn test_signed_area() {
        let data1 = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let data2 = vec![1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0];
        let a1 = signed_area(&data1, 0, 4, 2);
        let a2 = signed_area(&data2, 0, 4, 2);
        assert!(a1 == -a2);
    }

    #[test]
    fn test_deviation() {
        let data1 = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let tris = vec![0, 1, 2, 2, 3, 0];
        let hi: Vec<usize> = Vec::new();
        assert!(deviation(&data1, &hi, 2, &tris) == 0.0);
    }

    #[test]
    fn test_split_bridge_polygon() {
        let mut body = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let hole = vec![0.1, 0.1, 0.1, 0.2, 0.2, 0.2];
        body.extend(hole);
        let dims = 2;
        let (mut ll, _) = linked_list(&body, 0, body.len(), dims, true);
        assert!(ll.cycle_len(0) == body.len() / dims);
        let (left, right) = (0, 4);
        let np = split_bridge_polygon(&mut ll, left, right);
        assert!(ll.cycle_len(left) == 4);
        assert!(ll.cycle_len(np) == 5);
        // contrary to name, this should join the two cycles back together.
        let np2 = split_bridge_polygon(&mut ll, left, np);
        assert!(ll.cycle_len(np2) == 11);
        assert!(ll.cycle_len(left) == 11);
    }

    #[test]
    fn test_equals() {
        let dims = 2;

        let body = vec![0.0, 1.0, 0.0, 1.0];
        let (ll, _) = linked_list(&body, 0, body.len(), dims, true);
        assert!(equals(&ll.nodes[0], &ll.nodes[1]));

        let body = vec![2.0, 1.0, 0.0, 1.0];
        let (ll, _) = linked_list(&body, 0, body.len(), dims, true);
        assert!(!equals(&ll.nodes[0], &ll.nodes[1]));
    }

    #[test]
    fn test_area() {
        let dims = 2;
        let body = vec![4.0, 0.0, 4.0, 3.0, 0.0, 0.0]; // counterclockwise
        let (ll, _) = linked_list(&body, 0, body.len(), dims, true);
        assert!(area(&ll.nodes[0], &ll.nodes[1], &ll.nodes[2]) == -12.0);
        let body2 = vec![4.0, 0.0, 0.0, 0.0, 4.0, 3.0]; // clockwise
        let (ll2, _) = linked_list(&body2, 0, body2.len(), dims, true);
        // creation apparently modifies all winding to ccw
        assert!(area(&ll2.nodes[0], &ll2.nodes[1], &ll2.nodes[2]) == -12.0);
    }

    #[test]
    fn test_is_ear() {
        let dims = 2;
        let m = vec![0.0, 0.0, 0.5, 0.0, 1.0, 0.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dims, true);
        assert!(!is_ear(&ll, 0));
        assert!(!is_ear(&ll, 1));
        assert!(!is_ear(&ll, 2));

        let m = vec![0.0, 0.0, 0.5, 0.5, 1.0, 0.0, 0.5, 0.4];
        let (ll, _) = linked_list(&m, 0, m.len(), dims, true);
        assert!(is_ear(&ll, 0) == false);
        assert!(is_ear(&ll, 1) == true);
        assert!(is_ear(&ll, 2) == false);
        assert!(is_ear(&ll, 3) == true);

        let m = vec![0.0, 0.0, 0.5, 0.5, 1.0, 0.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dims, true);
        assert!(is_ear(&ll, 1));

        let m = vec![0.0, 0.0, 4.0, 0.0, 4.0, 3.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dims, true);
        assert!(is_ear(&ll, 1));
    }

    #[test]
    fn test_filter_points() {
        let dims = 2;

        let m = vec![0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let (mut ll1, _) = linked_list(&m, 0, m.len(), dims, true);
        let ll1len = ll1.nodes.len();
        let r1 = filter_points(&mut ll1, 0, ll1len - 1);
        dlog!(9, "{}", ll1.dump());
        assert!(ll1.cycle_len(r1) == 4);

        let n = vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let (mut ll2, _) = linked_list(&n, 0, n.len(), dims, true);
        let ll2len = ll2.nodes.len();
        let r2 = filter_points(&mut ll2, 0, ll2len - 1);
        dlog!(9, "{}", ll2.dump());
        assert!(ll2.cycle_len(r2) == 4);

        let n2 = vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let (mut ll22, _) = linked_list(&n2, 0, n2.len(), dims, true);
        let r32 = filter_points(&mut ll22, 0, 99);
        dlog!(9, "{}", ll2.dump());
        assert!(ll22.cycle_len(r32) != 4);

        let o = vec![0.0, 0.0, 0.5, 0.5, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let (mut ll3, _) = linked_list(&o, 0, o.len(), dims, true);
        let ll3len = ll3.nodes.len();
        let r3 = filter_points(&mut ll3, 0, ll3len - 1);
        dlog!(9, "{}", ll3.dump());
        assert!(ll3.cycle_len(r3) == 5);
    }

    #[test]
    fn test_earcut_linked() {
        let dim = 2;

        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let (mut ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let (mut tris, minx, miny, invsize) = (Vec::new(), 0.0, 0.0, 0.0);
        earcut_linked(&mut ll, 0, &mut tris, dim, minx, miny, invsize, 0);
        assert!(tris.len() == 6);

        let m = vec![0.0, 0.0, 0.5, 0.5, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let (mut ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let (mut tris, minx, miny, invsize) = (Vec::new(), 0.0, 0.0, 0.0);
        earcut_linked(&mut ll, 0, &mut tris, dim, minx, miny, invsize, 0);
        assert!(tris.len() == 9);

        let m = vec![0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let (mut ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let (mut tris, minx, miny, invsize) = (Vec::new(), 0.0, 0.0, 0.0);
        earcut_linked(&mut ll, 0, &mut tris, dim, minx, miny, invsize, 0);
        assert!(tris.len() == 9);
    }

    #[test]
    fn test_middle_inside() {
        let dim = 2;
        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        assert!(middle_inside(&ll, &node!(ll, 0), &node!(ll, 2)));
        assert!(middle_inside(&ll, &node!(ll, 1), &node!(ll, 3)));

        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.9, 0.1];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        assert!(!middle_inside(&ll, &node!(ll, 0), &node!(ll, 2)));
        assert!(middle_inside(&ll, &node!(ll, 1), &node!(ll, 3)));
    }

    #[test]
    fn test_locally_inside() {
        let dim = 2;
        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        assert!(locally_inside(&ll, &node!(ll, 0), &node!(ll, 0)));
        assert!(locally_inside(&ll, &node!(ll, 0), &node!(ll, 1)));
        assert!(locally_inside(&ll, &node!(ll, 0), &node!(ll, 2)));
        assert!(locally_inside(&ll, &node!(ll, 0), &node!(ll, 3)));

        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.9, 0.1];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        assert!(locally_inside(&ll, &node!(ll, 0), &node!(ll, 0)));
        assert!(locally_inside(&ll, &node!(ll, 0), &node!(ll, 1)));
        assert!(!locally_inside(&ll, &node!(ll, 0), &node!(ll, 2)));
        assert!(locally_inside(&ll, &node!(ll, 0), &node!(ll, 3)));
    }

    #[test]
    fn test_intersects_polygon() {
        let dim = 2;
        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);

        assert!(false == intersects_polygon(&ll, &node!(ll, 0), &node!(ll, 2)));
        assert!(false == intersects_polygon(&ll, &node!(ll, 2), &node!(ll, 0)));
        assert!(false == intersects_polygon(&ll, &node!(ll, 1), &node!(ll, 3)));
        assert!(false == intersects_polygon(&ll, &node!(ll, 3), &node!(ll, 1)));

        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.1, 0.1, 0.9, 1.0, 0.0, 1.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        dlog!(9, "{}", ll.dump());
        dlog!(
            5,
            "{}",
            intersects_polygon(&ll, &node!(ll, 0), &node!(ll, 2))
        );
        dlog!(
            5,
            "{}",
            intersects_polygon(&ll, &node!(ll, 2), &node!(ll, 0))
        );
    }

    #[test]
    fn test_intersects_itself() {
        let dim = 2;
        let m = vec![0.0, 0.0, 1.0, 0.0, 0.9, 0.9, 0.0, 1.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        macro_rules! ti {
            ($ok:expr,$a:expr,$b:expr,$c:expr,$d:expr) => {
                assert!(
                    $ok == pseudo_intersects(
                        &ll.nodes[$a],
                        &ll.nodes[$b],
                        &ll.nodes[$c],
                        &ll.nodes[$d]
                    )
                );
            };
        };
        ti!(false, 0, 2, 0, 1);
        ti!(false, 0, 2, 1, 2);
        ti!(false, 0, 2, 2, 3);
        ti!(false, 0, 2, 3, 0);
        ti!(true, 0, 2, 3, 1);
        ti!(true, 0, 2, 1, 3);
        ti!(true, 2, 0, 3, 1);
        ti!(true, 2, 0, 1, 3);
        ti!(false, 0, 1, 2, 3);
        ti!(false, 1, 0, 2, 3);
        ti!(false, 0, 0, 2, 3);
        ti!(false, 0, 1, 3, 2);
        ti!(false, 1, 0, 3, 2);

        ti!(true, 0, 2, 2, 0); // special cases
        ti!(true, 0, 2, 0, 2);

        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.1, 0.1, 0.9, 1.0, 0.0, 1.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        assert!(false == pseudo_intersects(&ll.nodes[3], &ll.nodes[4], &ll.nodes[0], &ll.nodes[2]));

        // special case
        assert!(true == pseudo_intersects(&ll.nodes[3], &ll.nodes[4], &ll.nodes[2], &ll.nodes[0]));
    }

    #[test]
    fn test_is_valid_diagonal() {
        let dim = 2;
        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.9, 0.1];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        assert!(!is_valid_diagonal(&ll, &ll.nodes[0], &ll.nodes[1]));
        assert!(!is_valid_diagonal(&ll, &ll.nodes[1], &ll.nodes[2]));
        assert!(!is_valid_diagonal(&ll, &ll.nodes[2], &ll.nodes[3]));
        assert!(!is_valid_diagonal(&ll, &ll.nodes[3], &ll.nodes[0]));
        assert!(!is_valid_diagonal(&ll, &ll.nodes[0], &ll.nodes[2]));
        assert!(is_valid_diagonal(&ll, &ll.nodes[1], &ll.nodes[3]));
        assert!(!is_valid_diagonal(&ll, &ll.nodes[2], &ll.nodes[0]));
        assert!(is_valid_diagonal(&ll, &ll.nodes[3], &ll.nodes[1]));
    }

    #[test]
    fn test_find_hole_bridge() {
        let dim = 2;

        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let hole = Node::new(0, 0.8, 0.8, NULL);
        assert!(0 == find_hole_bridge(&ll, &hole, 0));

        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.4, 0.5];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let hole = Node::new(0, 0.5, 0.5, NULL);
        assert!(4 == find_hole_bridge(&ll, &hole, 0));

        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, -0.4, 0.5];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let hole = Node::new(0, 0.5, 0.5, NULL);
        assert!(4 == find_hole_bridge(&ll, &hole, 0));

        let m = vec![
            0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, -0.1, 0.9, 0.1, 0.8, -0.1, 0.7, 0.1, 0.6, -0.1,
            0.5,
        ];
        let (ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let hole = Node::new(0, 0.5, 0.9, NULL);
        assert!(4 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.1, NULL);
        assert!(8 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.5, NULL);
        assert!(8 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.55, NULL);
        assert!(8 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.6, NULL);
        assert!(7 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.65, NULL);
        assert!(6 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.7, NULL);
        assert!(6 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.75, NULL);
        assert!(6 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.8, NULL);
        assert!(5 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.85, NULL);
        assert!(4 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.9, NULL);
        assert!(4 == find_hole_bridge(&ll, &hole, 0));
        let hole = Node::new(0, 0.2, 0.95, NULL);
        assert!(4 == find_hole_bridge(&ll, &hole, 0));
    }

    #[test]
    fn test_eliminate_hole() {
        let dims = 2;
        let mut body = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];

        let hole = vec![0.1, 0.1, 0.9, 0.1, 0.9, 0.9, 0.1, 0.9];
        let bodyend = body.len();
        body.extend(hole);
        let holestart = bodyend;
        let holeend = body.len();
        let (mut ll, _) = linked_list(&body, 0, bodyend, dims, true);
        linked_list_add_contour(&mut ll, &body, holestart, holeend, dims, false);
        assert!(ll.cycle_len(0) == 4);
        assert!(ll.cycle_len(5) == 4);
        eliminate_hole(&mut ll, holestart / dims, 0);
        assert!(ll.cycle_len(0) == 10);

        let hole = vec![0.2, 0.2, 0.8, 0.2, 0.8, 0.8, 0.2, 0.8];
        let bodyend = body.len();
        body.extend(hole);
        let holestart = bodyend;
        let holeend = body.len();
        linked_list_add_contour(&mut ll, &body, holestart, holeend, dims, false);
        assert!(ll.cycle_len(0) == 10);
        assert!(ll.cycle_len(5) == 10);
        assert!(ll.cycle_len(10) == 4);
        eliminate_hole(&mut ll, 10, 0);
        assert!(!ll.cycle_len(0) != 10);
        assert!(!ll.cycle_len(0) != 10);
        assert!(!ll.cycle_len(5) != 10);
        assert!(!ll.cycle_len(10) != 4);
        assert!(ll.cycle_len(0) == 16);
        assert!(ll.cycle_len(1) == 16);
        assert!(ll.cycle_len(10) == 16);
        assert!(ll.cycle_len(15) == 16);
    }

    #[test]
    fn test_cycle_len() {
        let dims = 2;
        let mut body = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.1, 0.1];

        let hole = vec![0.1, 0.1, 0.9, 0.1, 0.9, 0.9, 0.1, 0.9];
        let bodyend = body.len();
        body.extend(hole);
        let holestart = bodyend;
        let holeend = body.len();
        let (mut ll, _) = linked_list(&body, 0, bodyend, dims, true);
        linked_list_add_contour(&mut ll, &body, holestart, holeend, dims, false);

        let hole = vec![0.2, 0.2, 0.8, 0.2, 0.8, 0.8];
        let bodyend = body.len();
        body.extend(hole);
        let holestart = bodyend;
        let holeend = body.len();
        linked_list_add_contour(&mut ll, &body, holestart, holeend, dims, false);

        dlog!(5, "{}", ll.dump());
        dlog!(5, "{}", ll.cycles_report());
    }

    #[test]
    fn test_cycles_report() {
        let dims = 2;
        let mut body = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.1, 0.1];

        let hole = vec![0.1, 0.1, 0.9, 0.1, 0.9, 0.9, 0.1, 0.9];
        let bodyend = body.len();
        body.extend(hole);
        let holestart = bodyend;
        let holeend = body.len();
        let (mut ll, _) = linked_list(&body, 0, bodyend, dims, true);
        linked_list_add_contour(&mut ll, &body, holestart, holeend, dims, false);

        let hole = vec![0.2, 0.2, 0.8, 0.2, 0.8, 0.8];
        let bodyend = body.len();
        body.extend(hole);
        let holestart = bodyend;
        let holeend = body.len();
        linked_list_add_contour(&mut ll, &body, holestart, holeend, dims, false);

        dlog!(5, "{}", ll.dump());
        dlog!(5, "{}", ll.cycles_report());
    }

    #[test]
    fn test_eliminate_holes() {
        let dims = 2;
        let mut hole_indices: Vec<usize> = Vec::new();
        let mut body = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let (mut ll, _) = linked_list(&body, 0, body.len(), dims, true);
        let hole1 = vec![0.1, 0.1, 0.9, 0.1, 0.9, 0.9, 0.1, 0.9];
        let hole2 = vec![0.2, 0.2, 0.8, 0.2, 0.8, 0.8, 0.2, 0.8];
        hole_indices.push(body.len() / dims);
        hole_indices.push((body.len() + hole1.len()) / dims);
        body.extend(hole1);
        body.extend(hole2);

        eliminate_holes(&mut ll, &body, &hole_indices, 0, 2);

        assert!(ll.cycle_len(0) == body.len() / 2 + 2 + 2);
        assert!(ll.cycle_len(13) == body.len() / 2 + 2 + 2);

        let body = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let (mut ll, _) = linked_list(&body, 0, body.len(), dims, true);
        let hole_indices: Vec<usize> = Vec::new();
        assert!(3 == eliminate_holes(&mut ll, &body, &hole_indices, 3, 2));
        assert!(2 == eliminate_holes(&mut ll, &body, &hole_indices, 2, 2));
        assert!(1 == eliminate_holes(&mut ll, &body, &hole_indices, 1, 2));
        assert!(0 == eliminate_holes(&mut ll, &body, &hole_indices, 0, 2));
    }

    #[test]
    fn test_cure_local_intersections() {
        let dim = 2;
        // first test - it would be nice if it "detected" this but
        // the points are not 'local' enough to each other in the cycle
        let m = vec![
            0.0, 0.0, 1.0, 0.0, 1.1, 0.1, 0.9, 0.1, 1.0, 0.05, 1.0, 1.0, 0.0, 1.0,
        ];
        let (mut ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let mut triangles: Vec<usize> = Vec::new();
        cure_local_intersections(&mut ll, 0, &mut triangles, dim);
        assert!(ll.cycle_len(0) == 7);
        assert!(ll.freelist.len() == 0);
        assert!(triangles.len() == 0);

        // second test - we have three points that immediately cause
        // self intersection. so it should, in theory, detect and clean
        let m = vec![0.0, 0.0, 1.0, 0.0, 1.1, 0.1, 1.1, 0.0, 1.0, 1.0, 0.0, 1.0];
        let (mut ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let mut triangles: Vec<usize> = Vec::new();
        cure_local_intersections(&mut ll, 0, &mut triangles, dim);
        assert!(ll.cycle_len(0) == 4);
        assert!(ll.freelist.len() == 2);
        assert!(triangles.len() == 3);
    }

    #[test]
    fn test_split_earcut() {
        let m = vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];

        let (minx, miny, maxx, maxy) = (0.0, 0.0, 1.0, 1.0);
        let invsize = calc_invsize(minx, miny, maxx, maxy);
        let dim = 2;
        let (mut ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let start = 0;
        let mut triangles: Vec<usize> = Vec::new();
        split_earcut(&mut ll, start, &mut triangles, dim, minx, miny, invsize);
        assert!(triangles.len() == 6);
        assert!(ll.nodes.len() == 6);
        assert!(ll.freelist.len() == 2);

        let m = vec![
            0.0, 0.0, 1.0, 0.0, 1.5, 0.5, 2.0, 0.0, 3.0, 0.0, 3.0, 1.0, 2.0, 1.0, 1.5, 0.6, 1.0,
            1.0, 0.0, 1.0,
        ];
        let (minx, miny, maxx, maxy) = (0.0, 0.0, 1.0, 1.0);
        let invsize = calc_invsize(minx, miny, maxx, maxy);
        let dim = 2;
        let (mut ll, _) = linked_list(&m, 0, m.len(), dim, true);
        let start = 0;
        let mut triangles: Vec<usize> = Vec::new();
        split_earcut(&mut ll, start, &mut triangles, dim, minx, miny, invsize);
        assert!(ll.nodes.len() == 12);
    }

    #[test]
    fn test_flatten() {
        let data: Vec<Vec<Vec<f64>>> = vec![
            vec![
                vec![0.0, 0.0],
                vec![1.0, 0.0],
                vec![1.0, 1.0],
                vec![0.0, 1.0],
            ],
            vec![
                vec![0.1, 0.1],
                vec![0.9, 0.1],
                vec![0.9, 0.9],
                vec![0.1, 0.9],
            ],
            vec![
                vec![0.2, 0.2],
                vec![0.8, 0.2],
                vec![0.8, 0.8],
                vec![0.2, 0.8],
            ],
        ];
        let (coords, hole_indices, dim) = flatten(&data);
        assert!(coords.len() == 24);
        assert!(hole_indices.len() == 2);
        assert!(hole_indices[0] == 4);
        assert!(hole_indices[1] == 8);
        assert!(dim == 2);
    }

    #[test]
    fn test_iss45() {
        let data = vec![
            vec![
                vec![10.0, 10.0],
                vec![25.0, 10.0],
                vec![25.0, 40.0],
                vec![10.0, 40.0],
            ],
            vec![vec![15.0, 30.0], vec![20.0, 35.0], vec![10.0, 40.0]],
            vec![vec![15.0, 15.0], vec![15.0, 20.0], vec![20.0, 15.0]],
        ];
        let (coords, hole_indices, dim) = flatten(&data);
        let triangles = earcut(&coords, &hole_indices, dim);
        assert!(triangles.len() > 0);
    }

}
