use std::{io::Write, marker::PhantomData};

// A physical "object" is either a Blob (an immutable vector of bytes)
// or a Tree (an immutable vector of "Handles", defined below).
type Blob = [u8];
type Tree<T> = [T];

// A Pointer is an opaque pointer to an object (in this case, a 192-bit number).
// In practice this is either a pointer to the object in memory or a canonical hash of the contents.
type Pointer<T> = (u64, u64, u64, PhantomData<T>);

// A Blob "Name" identifies a Blob and its length, either by containing
// its contents directly in the Name (a Literal) or via Pointer.
#[derive(Copy, Clone)]
enum BlobName {
    Literal(([u8; 30], u8)),
    Name((Pointer<Blob>, usize)),
}

const PAGE_SIZE: usize = 65536; // Units of estimated memory "footprint" (64 KiB)
const HANDLE_SIZE: usize = 32; // Size of a Handle in memory (256 bits)

// A Tree "Name" identifies a Tree, its length, and an estimate of its memory "footprint"
// (number of pages consumed by the Tree itself plus the footprint of its accessible Refs).
// It also records whether the Name is "eq" (can be compared against other Tree Names for equality)
// and whether it is "tagged" (meaning the procedure that authored it is named in the first element).
//
// A Tree Name is "eq" iff all of the Handles of the Tree are "eq".
//
// At runtime, the Tree element type is always the most general "Handle", but it is
// parametrized here to allow tighter guarantees a priori.
#[derive(Copy, Clone)]
struct TreeName<T: HandleType = Handle> {
    name: Pointer<Tree<T>>,
    size: u32,
    footprint: u32,
    eq: bool,
    tag: bool,
}

// A Ref is a reference to an inaccessible physical object (Blob or Tree).
// Only the Name metadata (e.g. size, footprint, tag, eq) can be accessed.
// A Ref's Tree may never have been looked at after creation, so it must be
// the general type (in a system without runtime Tree types).
#[derive(Copy, Clone)]
enum Ref {
    Blob(BlobName),
    Tree(TreeName),
}

// An Object is a reference to an accessible physical object (Blob or Tree).
// In addition to the Name metadata, the object itself can be accessed.
#[derive(Copy, Clone)]
enum Object<T: HandleType = Handle> {
    Blob(BlobName),
    Tree(TreeName<T>),
}

// Objects and Refs are "Data".
#[derive(Copy, Clone)]
enum Data<T: HandleType = Handle> {
    Ref(Ref),
    Object(Object<T>),
}

// A Thunk is an opaque reference to something that has yet to be evaluated. It is either described as:
// - an identification (which internally identifies the exact output, but opaquely)
// - a selection (of a particular element or subrange of a Blob or Tree)
// - an application (of a function to arguments).
// Within a Fix function, all Thunks are indistinguishable.
#[derive(Copy, Clone)]
enum Thunk {
    Identification(Data),
    Selection(TreeName),
    Application(TreeName),
}

// A Value is a Thunk or Data where every accessible object is also a Value.
// This is only used for type-checking this model.
#[derive(Copy, Clone)]
enum Value {
    Data(Data<Value>),
    Thunk(Thunk),
}

// A RuntimeValue is Thunk or (unrestricted) Data.
#[derive(Copy, Clone)]
enum RuntimeValue {
    Data(Data),
    Thunk(Thunk),
}

// An Encode (explicit named computation on data or Encodes) is an instruction requesting that
// a Thunk be "forced" and replaced with its result, optionally with a particular accessibility.
#[derive(Copy, Clone)]
struct Encode {
    thunk: Thunk,
    accessibility: Option<bool>,
}

// A Handle is the element type of a Tree, intended to be storable in a 256-bit register.
// There are three variants: Data, Thunk, and Encode.
#[derive(Copy, Clone)]
enum Handle {
    Data(Data),
    Thunk(Thunk),
    Encode(Encode),
}

// The result of most Fix operations: either a handle of some kind,
// or a fatal trap (expressed as Fix data).
type Result<T> = std::result::Result<T, Data>;

// Fix operations: apply, select, think, execute, and eval.

// Apply a function to arguments, as described by an evaluated "combination":
// a tree that includes the resource limits, the function, and the arguments/environment.
// The evaluated combination (the input) will never contain any accessible Encodes.
// The function can return any Value it wants (it can't return an Encode,
// but it can return a Tree containing accessible Encodes).
fn apply(_evaluated_combination: TreeName<Value>) -> Result<RuntimeValue> {
    // must enforce that the type returned by a Fix procedure actually is a RuntimeValue
    unimplemented!("apply")
}

// Select data as specified, without loading or evaluating anything not needed.
// The specification language is TBD, but will permit:
// - fetching a byte range of a Blob
// - fetching a single element of a Tree
// - fetching a subrange of a Tree
// - truncating the output elements to be empty
//   (to permit discovery of element types without unnecessary accessible data)
fn select(_spec: TreeName) -> Result<RuntimeValue> {
    // must enforce that the type returned by a Fix procedure actually is a RuntimeValue
    unimplemented!("select")
}

// Execute one step of the evaluation of a Thunk. This might produce another Thunk.
fn think(thunk: Thunk) -> Result<RuntimeValue> {
    match thunk {
        Thunk::Application(combination) => apply(combination.try_map(eval)?),
        Thunk::Selection(spec) => select(spec),
        Thunk::Identification(x) => Ok(RuntimeValue::Data(x)),
    }
}

fn make_err(str: &str) -> Result<Data> {
    let mut buf = [0u8; 30];
    let mut out: &mut [u8] = &mut buf;
    out.write_all(str.as_bytes()).unwrap();
    Err(Data::Ref(Ref::Blob(BlobName::Literal((
        buf,
        str.as_bytes().len() as u8,
    )))))
}

// Execute an Encode, producing Data.
// The Thunk is thinked until no more thoughts arrive (i.e. it's Data).
// Then, if requested, the Data accessibility is adjusted.
fn execute(e: Encode) -> Result<Data> {
    match e {
        Encode {
            mut thunk,
            accessibility,
        } => {
            let data = loop {
                match think(thunk)? {
                    RuntimeValue::Thunk(thought) => thunk = thought,
                    RuntimeValue::Data(x) => break x,
                }
            };
            Ok(match accessibility {
                None => data,
                Some(true) => Data::Object(data.lift()),
                Some(false) => Data::Ref(data.lower()),
            })
        }
    }
}

// Evaluate a Handle to a Value (a data structure with no accessible Encodes).
// Any Encodes are executed, and accessible Trees are recursed into. Everything else is self-evaluating.
// The result is a Value: no accessible Encodes.
fn eval(h: Handle) -> Result<Value> {
    Ok(match h {
        Handle::Encode(e) => eval(Handle::Data(execute(e)?))?,
        Handle::Data(d) => Value::Data(match d {
            Data::Object(Object::Tree(x)) => Data::Object(Object::Tree(x.try_map(eval)?)),
            Data::Object(Object::Blob(x)) => Data::Object(Object::Blob(x)),
            Data::Ref(x) => Data::Ref(x),
        }),
        Handle::Thunk(thunk) => Value::Thunk(thunk),
    })
}

// impl blocks for Names, Refs, Data, Value, and Handle

// Associated functions of Blob and Tree Names:
// - load (Name -> object)
// - name & create (object -> Name)
// - size & footprint (Name -> usize)
//
// TreeNames also support `try_map`, which maps a function over the elements to create a new Tree,
// as well as `relax`, which converts a TreeName of more-restrictive Handles to a general Treename.
impl BlobName {
    fn load(&self) -> &Blob {
        match self {
            BlobName::Literal((storage, length)) => &storage[0..*length as usize],
            BlobName::Name((_, _)) => unimplemented!("load Blob from Pointer"),
        }
    }

    fn name(_blob: &Blob) -> Self {
        unimplemented!("BlobName::name")
    }

    fn create(_blobdata: Vec<u8>) -> Self {
        unimplemented!("BlobName::create")
    }

    fn size(&self) -> usize {
        match self {
            BlobName::Literal((_, length)) => *length as usize,
            BlobName::Name((_, length)) => *length,
        }
    }

    fn footprint(&self) -> u32 {
        self.size().div_ceil(PAGE_SIZE) as u32
    }
}

impl<T: HandleType> TreeName<T> {
    fn load(&self) -> &Tree<T> {
        unimplemented!("load Tree from Pointer")
    }

    fn name(_tree: &Tree<T>) -> Self {
        unimplemented!("TreeName::name")
    }

    fn create(treedata: Vec<T>) -> Self {
        let _size = treedata.len() as u32;
        let _footprint = (treedata.len() * HANDLE_SIZE).div_ceil(PAGE_SIZE) as u32
            + treedata
                .iter()
                .fold(0, |acc: u32, elem| acc.saturating_add(elem.footprint()));
        let _eq = treedata.iter().all(|h| h.is_eq());

        unimplemented!("TreeName::create")
    }

    fn size(&self) -> usize {
        self.size as usize
    }

    fn footprint(&self) -> u32 {
        self.footprint
    }

    fn try_map<FuncType, TgT: HandleType>(&self, f: FuncType) -> Result<TreeName<TgT>>
    where
        FuncType: Fn(T) -> Result<TgT>,
    {
        self.load()
            .iter()
            .map(|h| f(*h))
            .collect::<Result<Vec<TgT>>>()
            .map(|vec| TreeName {
                tag: self.tag,
                ..TreeName::create(vec)
            })
    }

    fn relax(self) -> TreeName {
        TreeName {
            tag: self.tag,
            ..TreeName::create(
                self.load()
                    .iter()
                    .map(|h| h.relax())
                    .collect::<Vec<Handle>>(),
            )
        }
    }
}

// Blob Names can always be compared for equality.
// The Names are equal iff the underlying Blobs are.
impl PartialEq for BlobName {
    fn eq(&self, _other: &Self) -> bool {
        todo!("equality of BlobNames");
    }
}

// Tree Names can be compared for equality if both are "eq"
// (which means all of the Handle elements of each Tree are "eq").
// The Names are equal iff the underlying Trees are.
impl PartialEq for TreeName {
    fn eq(&self, other: &Self) -> bool {
        if self.tag != other.tag {
            return false;
        }
        match (self.eq, other.eq) {
            (true, true) => todo!("equality of Eq TreeNames"),
            _ => false,
        }
    }
}

// Associated functions of Refs: is_eq, lift
impl Ref {
    // Is the Ref eq? (Blobs always are, Trees are iff every element is)
    fn is_eq(&self) -> bool {
        match self {
            Ref::Blob(_) => true,
            Ref::Tree(t) => t.eq,
        }
    }

    // "lift" a Ref (make it accessible by loading the underlying object)
    fn lift(&self) -> Object {
        match self {
            Ref::Blob(x) => Object::Blob(BlobName::name(x.load())),
            Ref::Tree(x) => Object::Tree(TreeName {
                tag: x.tag,
                ..TreeName::name(x.load())
            }),
        }
    }
}

// Associated functions of Objects: is_eq, lower, relax
impl<T: HandleType> Object<T> {
    // Is the Ref eq? (Blobs always are, Trees are iff every element is)
    fn is_eq(&self) -> bool {
        self.lower().is_eq()
    }

    // "lower" an Object (make it inaccessible)
    fn lower(&self) -> Ref {
        match *self {
            Object::Blob(x) => Ref::Blob(x),
            Object::Tree(x) => Ref::Tree(x.relax()),
        }
    }

    fn relax(self) -> Object {
        match self {
            Object::Blob(x) => Object::Blob(x),
            Object::Tree(x) => Object::Tree(x.relax()),
        }
    }
}

// Associated functions of Data: lift, lower, is_eq, footprint
// These dispatch to the underlying Object or Ref.
impl<T: HandleType> Data<T> {
    fn lift(&self) -> Object {
        match self {
            Data::Object(x) => x.relax(),
            Data::Ref(x) => x.lift(),
        }
    }

    fn lower(&self) -> Ref {
        match *self {
            Data::Object(x) => x.lower(),
            Data::Ref(x) => x,
        }
    }

    fn is_eq(&self) -> bool {
        match self {
            Data::Object(x) => x.is_eq(),
            Data::Ref(x) => x.is_eq(),
        }
    }

    fn footprint(&self) -> u32 {
        match self {
            Data::Object(Object::Blob(x)) => x.footprint(),
            Data::Object(Object::Tree(x)) => x.footprint(),
            _ => 0,
        }
    }
}

trait HandleType: Copy + Clone + PartialEq {
    fn is_eq(&self) -> bool;
    fn footprint(&self) -> u32;
    fn relax(self) -> Handle;
}

// Associated functions of Handle: is_eq, footprint, eq, from(Value)
impl HandleType for Handle {
    fn is_eq(&self) -> bool {
        match self {
            Handle::Data(x) => x.is_eq(),
            _ => false,
        }
    }

    fn footprint(&self) -> u32 {
        match self {
            Handle::Data(x) => x.footprint(),
            _ => 0,
        }
    }

    fn relax(self) -> Handle {
        self
    }
}

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Handle::Data(x), Handle::Data(y)) => x == y,
            _ => false,
        }
    }
}

impl PartialEq for Data {
    fn eq(&self, other: &Self) -> bool {
        match (self.lower(), other.lower()) {
            (Ref::Blob(x), Ref::Blob(y)) => x == y,
            (Ref::Tree(x), Ref::Tree(y)) => x == y,
            _ => false,
        }
    }
}

impl HandleType for Value {
    fn is_eq(&self) -> bool {
        match self {
            Value::Data(x) => x.is_eq(),
            _ => false,
        }
    }

    fn footprint(&self) -> u32 {
        match self {
            Value::Data(x) => x.footprint(),
            _ => 0,
        }
    }

    fn relax(self) -> Handle {
        match self {
            Value::Thunk(x) => Handle::Thunk(x),
            Value::Data(Data::Ref(x)) => Handle::Data(Data::Ref(x)),
            Value::Data(Data::Object(Object::Blob(x))) => {
                Handle::Data(Data::Object(Object::Blob(x)))
            }
            Value::Data(Data::Object(Object::Tree(x))) => {
                Handle::Data(Data::Object(Object::Tree(x.relax())))
            }
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.relax() == other.relax()
    }
}

fn main() {
    println!("Hello, world!");
}
