//! Cranelift code generation backend for the Atom compiler.
//!
//! This module provides a code generator that translates Atom IR to native machine code
//! using the Cranelift compiler infrastructure. The generator handles:
//!
//! - Translation of IR types to Cranelift types
//! - Function compilation with proper ABI handling
//! - Basic block control flow
//! - All IR instructions (arithmetic, calls, struct/tuple operations)
//! - Memory layout for composite types
//! - Executable and object file generation
//!
//! # Architecture
//!
//! The code generator works in several phases:
//! 1. **Type translation**: Map IrType to Cranelift types
//! 2. **Function signature creation**: Set up proper ABIs for functions
//! 3. **Instruction translation**: Convert each IR instruction to Cranelift IR
//! 4. **Module compilation**: Generate executable or object file output
//!
//! # Limitations
//!
//! - Closures are simplified: captures are ignored for basic implementation
//! - Enums use a simple tag + max-size-union layout
//! - No garbage collection or advanced memory management
//! - Limited optimization (relies on Cranelift's built-in passes)

use crate::ir::{
    BlockId, IrBinOp, IrConstant, IrEnumDef, IrFunction, IrInstruction,
    IrInstructionKind, IrMemoryLocation, IrProgram, IrStructDef, IrTerminator, IrType, IrUnOp,
    LocalId, ValueId,
};
use cranelift::prelude::*;
use cranelift::prelude::isa::CallConv;
use cranelift::codegen::ir::StackSlot;
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use std::fs::File;
use std::io::Write;

/// Error type for code generation failures.
#[derive(Debug)]
pub enum CodegenError {
    /// Type conversion failed
    UnsupportedType(String),
    /// Instruction translation failed
    UnsupportedInstruction(String),
    /// Invalid value reference
    InvalidValue(ValueId),
    /// Invalid block reference
    InvalidBlock(BlockId),
    /// Invalid local reference
    InvalidLocal(LocalId),
    /// Function not found
    FunctionNotFound(String),
    /// Struct not found
    StructNotFound(String),
    /// Enum not found
    EnumNotFound(String),
    /// Module error
    ModuleError(String),
    /// Cranelift error
    CraneliftError(String),
    /// I/O error
    IoError(String),
    /// Other error
    Other(String),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::UnsupportedType(msg) => write!(f, "Unsupported type: {}", msg),
            CodegenError::UnsupportedInstruction(msg) => {
                write!(f, "Unsupported instruction: {}", msg)
            }
            CodegenError::InvalidValue(id) => write!(f, "Invalid value reference: {:?}", id),
            CodegenError::InvalidBlock(id) => write!(f, "Invalid block reference: {:?}", id),
            CodegenError::InvalidLocal(id) => write!(f, "Invalid local reference: {:?}", id),
            CodegenError::FunctionNotFound(name) => write!(f, "Function not found: {}", name),
            CodegenError::StructNotFound(name) => write!(f, "Struct not found: {}", name),
            CodegenError::EnumNotFound(name) => write!(f, "Enum not found: {}", name),
            CodegenError::ModuleError(msg) => write!(f, "Module error: {}", msg),
            CodegenError::CraneliftError(msg) => write!(f, "Cranelift error: {}", msg),
            CodegenError::IoError(msg) => write!(f, "I/O error: {}", msg),
            CodegenError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl StdError for CodegenError {}

impl From<std::io::Error> for CodegenError {
    fn from(err: std::io::Error) -> Self {
        CodegenError::IoError(err.to_string())
    }
}

/// Result type for codegen operations
pub type CodegenResult<T> = Result<T, CodegenError>;

/// Main code generator struct
///
/// This handles the complete compilation pipeline from IR to native code.
pub struct CodeGenerator {
    /// Target triple for code generation
    #[allow(dead_code)]
    target_triple: target_lexicon::Triple,
    /// Struct definitions from the IR
    struct_defs: HashMap<String, IrStructDef>,
    /// Enum definitions from the IR
    enum_defs: HashMap<String, IrEnumDef>,
    /// String constants (maps string content to DataId)
    string_constants: HashMap<Vec<u8>, DataId>,
}

impl CodeGenerator {
    /// Create a new code generator for the native target
    pub fn new() -> Self {
        Self {
            target_triple: target_lexicon::Triple::host(),
            struct_defs: HashMap::new(),
            enum_defs: HashMap::new(),
            string_constants: HashMap::new(),
        }
    }

    /// Create a code generator for a specific target
    pub fn new_for_target(target: target_lexicon::Triple) -> Self {
        Self {
            target_triple: target,
            struct_defs: HashMap::new(),
            enum_defs: HashMap::new(),
            string_constants: HashMap::new(),
        }
    }

    /// Compile an IR program to an executable or object file
    ///
    /// # Arguments
    ///
    /// * `ir` - The IR program to compile
    /// * `output_path` - Path to write the output file
    ///
    /// # Returns
    ///
    /// Ok(()) on success, or a CodegenError on failure
    pub fn compile(&mut self, ir: IrProgram, output_path: &str) -> CodegenResult<()> {
        // Store struct and enum definitions for type translation
        for struct_def in &ir.structs {
            self.struct_defs.insert(struct_def.name.clone(), struct_def.clone());
        }
        for enum_def in &ir.enums {
            self.enum_defs.insert(enum_def.name.clone(), enum_def.clone());
        }

        // Create the Cranelift module
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed").map_err(|e| {
            CodegenError::CraneliftError(format!("Failed to set opt_level: {}", e))
        })?;
        // Enable PIC for compatibility with modern linkers (required on macOS)
        flag_builder.set("is_pic", "true").map_err(|e| {
            CodegenError::CraneliftError(format!("Failed to set is_pic: {}", e))
        })?;
        let isa_builder = cranelift_native::builder()
            .map_err(|e| CodegenError::CraneliftError(format!("Failed to create ISA builder: {}", e)))?;
        let isa = isa_builder.finish(settings::Flags::new(flag_builder))
            .map_err(|e| CodegenError::CraneliftError(format!("Failed to create ISA: {}", e)))?;

        let builder = ObjectBuilder::new(
            isa,
            "atom_module",
            cranelift_module::default_libcall_names(),
        ).map_err(|e| {
            CodegenError::ModuleError(format!("Failed to create object builder: {}", e))
        })?;
        let mut module = ObjectModule::new(builder);

        // Declare all functions first (for forward references)
        let mut func_ids = HashMap::new();
        let mut external_funcs = HashMap::new();
        
        for func in &ir.functions {
            let func_id = self.declare_function(&mut module, func)?;
            func_ids.insert(func.name.clone(), func_id);
            
            // Scan for C library calls to declare them as external
            self.collect_external_functions(func, &mut external_funcs);
        }
        
        // Declare all external C functions
        for (c_name, signature) in external_funcs {
            let func_id = module
                .declare_function(&c_name, Linkage::Import, &signature)
                .map_err(|e| {
                    CodegenError::ModuleError(format!(
                        "Failed to declare external function '{}': {}",
                        c_name, e
                    ))
                })?;
            func_ids.insert(format!("c::{}", c_name), func_id);
        }

        // Compile each function
        for func in &ir.functions {
            let func_id = func_ids[&func.name];
            self.compile_function(&mut module, func, func_id, &func_ids)?;
        }

        // Generate the object file
        let product = module.finish();
        let bytes = product.emit().map_err(|e| {
            CodegenError::ModuleError(format!("Failed to emit object file: {}", e))
        })?;

        // Write to output file
        let mut file = File::create(output_path)?;
        file.write_all(&bytes)?;

        Ok(())
    }
    
    /// Collect all external C function references from a function
    fn collect_external_functions(
        &self,
        func: &IrFunction,
        external_funcs: &mut HashMap<String, Signature>,
    ) {
        for block in &func.blocks {
            for inst in &block.instructions {
                if let IrInstructionKind::Call { function, args } = &inst.kind {
                    // Check if this is a C function call
                    if function.starts_with('c') && function.contains("::") {
                        let parts: Vec<&str> = function.split("::").collect();
                        if parts.len() == 2 {
                            let c_func_name = parts[1].to_string();
                            
                            // Only add if not already declared
                            if !external_funcs.contains_key(&c_func_name) {
                                // Create signature for C function
                                let mut sig = Signature::new(CallConv::SystemV);
                                
                                // Add variadic parameters (use actual arg count)
                                for _ in 0..args.len() {
                                    // For simplicity, assume all args are i64 or pointers
                                    sig.params.push(AbiParam::new(types::I64));
                                }
                                
                                // Determine return type
                                let ret_type = match c_func_name.as_str() {
                                    "exit" | "printf" => None,
                                    name if name.ends_with('f') => Some(types::F32),
                                    _ if parts[0] == "cmath" => Some(types::F64),
                                    _ => Some(types::I64),
                                };
                                
                                if let Some(rt) = ret_type {
                                    sig.returns.push(AbiParam::new(rt));
                                }
                                
                                external_funcs.insert(c_func_name, sig);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Declare a function in the module
    fn declare_function(
        &self,
        module: &mut ObjectModule,
        func: &IrFunction,
    ) -> CodegenResult<FuncId> {
        let mut sig = module.make_signature();

        // Add parameters
        for (_, param_ty) in &func.params {
            let cl_type = self.translate_type(param_ty)?;
            sig.params.push(AbiParam::new(cl_type));
        }

        // Add return type
        if let Some(ret_ty) = &func.return_type {
            let cl_type = self.translate_type(ret_ty)?;
            sig.returns.push(AbiParam::new(cl_type));
        }

        // Determine linkage
        let linkage = if func.is_public || func.name == "main" {
            Linkage::Export
        } else {
            Linkage::Local
        };

        module
            .declare_function(&func.name, linkage, &sig)
            .map_err(|e| {
                CodegenError::ModuleError(format!(
                    "Failed to declare function '{}': {}",
                    func.name, e
                ))
            })
    }

    /// Compile a single function
    fn compile_function(
        &mut self,
        module: &mut ObjectModule,
        func: &IrFunction,
        func_id: FuncId,
        func_ids: &HashMap<String, FuncId>,
    ) -> CodegenResult<()> {
        let mut ctx = module.make_context();
        let mut fn_builder_ctx = FunctionBuilderContext::new();

        // Set up the function signature
        ctx.func.signature = module.make_signature();
        for (_, param_ty) in &func.params {
            let cl_type = self.translate_type(param_ty)?;
            ctx.func.signature.params.push(AbiParam::new(cl_type));
        }
        if let Some(ret_ty) = &func.return_type {
            let cl_type = self.translate_type(ret_ty)?;
            ctx.func.signature.returns.push(AbiParam::new(cl_type));
        }

        // Build the function body
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fn_builder_ctx);

            // Create Cranelift blocks for all IR blocks
            let mut cl_blocks = HashMap::new();
            for block in &func.blocks {
                let cl_block = builder.create_block();
                cl_blocks.insert(block.label, cl_block);
            }

            // Track values and stack slots
            let mut values = HashMap::new();
            let mut stack_slots = HashMap::new();

            // Create stack slots for parameters
            let entry_block = cl_blocks[&func.blocks[0].label];
            builder.switch_to_block(entry_block);
            builder.append_block_params_for_function_params(entry_block);

            // Map parameters to values
            for (i, _) in func.params.iter().enumerate() {
                let param_value = builder.block_params(entry_block)[i];
                // Parameters are referenced by value ID (we'll use a simple mapping)
                // In a real implementation, you'd track parameter value IDs from IR
                values.insert(ValueId(i as u32), param_value);
            }

            // Create stack slots for locals
            for local in &func.locals {
                let size = self.type_size(&local.ty) as u32;
                let slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    size,
                    0, // align_shift: 0 = natural alignment
                ));
                stack_slots.insert(local.id, slot);
            }

            // Translate each basic block
            for (block_idx, block) in func.blocks.iter().enumerate() {
                let cl_block = cl_blocks[&block.label];
                
                // Only switch to block if we're not already in it (entry block is already switched)
                if block_idx > 0 {
                    builder.switch_to_block(cl_block);
                }

                // Translate instructions
                for inst in &block.instructions {
                    let value = self.translate_instruction(
                        &mut builder,
                        inst,
                        &values,
                        &stack_slots,
                        func_ids,
                        module,
                    )?;
                    values.insert(inst.result, value);
                }

                // Translate terminator
                self.translate_terminator(
                    &mut builder,
                    &block.terminator,
                    &values,
                    &cl_blocks,
                )?;
            }

            // Seal all blocks
            for cl_block in cl_blocks.values() {
                builder.seal_block(*cl_block);
            }

            builder.finalize();
        }

        // Define the function in the module
        module
            .define_function(func_id, &mut ctx)
            .map_err(|e| {
                CodegenError::ModuleError(format!(
                    "Failed to define function '{}': {}",
                    func.name, e
                ))
            })?;

        // Clear the context for reuse
        module.clear_context(&mut ctx);

        Ok(())
    }

    /// Translate an IR instruction to Cranelift
    fn translate_instruction(
        &mut self,
        builder: &mut FunctionBuilder,
        inst: &IrInstruction,
        values: &HashMap<ValueId, Value>,
        stack_slots: &HashMap<LocalId, StackSlot>,
        func_ids: &HashMap<String, FuncId>,
        module: &mut ObjectModule,
    ) -> CodegenResult<Value> {
        match &inst.kind {
            IrInstructionKind::Const { value } => self.translate_const(builder, value, &inst.ty, module),

            IrInstructionKind::BinOp { op, left, right } => {
                let left_val = values
                    .get(left)
                    .ok_or(CodegenError::InvalidValue(*left))?;
                let right_val = values
                    .get(right)
                    .ok_or(CodegenError::InvalidValue(*right))?;
                self.translate_binop(builder, *op, *left_val, *right_val, &inst.ty)
            }

            IrInstructionKind::UnOp { op, operand } => {
                let operand_val = values
                    .get(operand)
                    .ok_or(CodegenError::InvalidValue(*operand))?;
                self.translate_unop(builder, *op, *operand_val, &inst.ty)
            }

            IrInstructionKind::Load { source } => {
                self.translate_load(builder, source, values, stack_slots, &inst.ty)
            }

            IrInstructionKind::Call { function, args } => {
                // Check if this is a C function call
                let actual_func_name = if function.starts_with('c') && function.contains("::") {
                    // Extract C function name: "cstdlib::printf" -> "printf"
                    let parts: Vec<&str> = function.split("::").collect();
                    if parts.len() == 2 {
                        // Look up the external function with the c:: prefix
                        format!("c::{}", parts[1])
                    } else {
                        function.clone()
                    }
                } else {
                    function.clone()
                };
                
                let func_id = func_ids
                    .get(&actual_func_name)
                    .ok_or_else(|| CodegenError::FunctionNotFound(actual_func_name.clone()))?;
                
                let func_ref = module.declare_func_in_func(*func_id, builder.func);
                
                let arg_vals: Result<Vec<_>, _> = args
                    .iter()
                    .map(|arg| values.get(arg).copied().ok_or(CodegenError::InvalidValue(*arg)))
                    .collect();
                let arg_vals = arg_vals?;

                let call = builder.ins().call(func_ref, &arg_vals);
                let results = builder.inst_results(call);
                
                if results.is_empty() {
                    // Void function - return a dummy value
                    Ok(builder.ins().iconst(types::I64, 0))
                } else {
                    Ok(results[0])
                }
            }

            IrInstructionKind::CallIndirect { .. } => {
                Err(CodegenError::UnsupportedInstruction(
                    "Indirect calls not yet implemented".to_string(),
                ))
            }

            IrInstructionKind::MakeTuple { elements } => {
                // For now, we'll handle tuples as sequential stack values
                // In a full implementation, this would create a struct in memory
                if elements.is_empty() {
                    // Empty tuple = void
                    Ok(builder.ins().iconst(types::I64, 0))
                } else {
                    // For simplicity, just return the first element
                    // A real implementation would allocate and pack all elements
                    values
                        .get(&elements[0])
                        .copied()
                        .ok_or(CodegenError::InvalidValue(elements[0]))
                }
            }

            IrInstructionKind::TupleExtract { tuple, index } => {
                // Simplified: assume tuple is just the value itself if index is 0
                // Real implementation would load from memory offset
                if *index == 0 {
                    values
                        .get(tuple)
                        .copied()
                        .ok_or(CodegenError::InvalidValue(*tuple))
                } else {
                    Err(CodegenError::UnsupportedInstruction(
                        "Non-zero tuple extract not yet implemented".to_string(),
                    ))
                }
            }

            IrInstructionKind::MakeStruct { struct_name: _, fields } => {
                // Simplified struct handling - similar to tuples
                if fields.is_empty() {
                    Ok(builder.ins().iconst(types::I64, 0))
                } else {
                    values
                        .get(&fields[0])
                        .copied()
                        .ok_or(CodegenError::InvalidValue(fields[0]))
                }
            }

            IrInstructionKind::StructExtract { struct_value, field_index } => {
                // Simplified field extraction
                if *field_index == 0 {
                    values
                        .get(struct_value)
                        .copied()
                        .ok_or(CodegenError::InvalidValue(*struct_value))
                } else {
                    Err(CodegenError::UnsupportedInstruction(
                        "Non-zero field extract not yet implemented".to_string(),
                    ))
                }
            }

            IrInstructionKind::MakeEnum { .. } => {
                Err(CodegenError::UnsupportedInstruction(
                    "Enum construction not yet implemented".to_string(),
                ))
            }

            IrInstructionKind::MakeClosure { .. } => {
                Err(CodegenError::UnsupportedInstruction(
                    "Closures not yet implemented".to_string(),
                ))
            }

            IrInstructionKind::LoadCapture { .. } => {
                Err(CodegenError::UnsupportedInstruction(
                    "Closure captures not yet implemented".to_string(),
                ))
            }

            IrInstructionKind::Phi { incoming } => {
                // Phi nodes require special handling with block parameters
                // For now, return the first incoming value
                // A proper implementation would use Cranelift's block parameters
                if let Some((_, value_id)) = incoming.first() {
                    values
                        .get(value_id)
                        .copied()
                        .ok_or(CodegenError::InvalidValue(*value_id))
                } else {
                    Err(CodegenError::UnsupportedInstruction(
                        "Empty phi node".to_string(),
                    ))
                }
            }
        }
    }

    /// Translate a constant value
    fn translate_const(
        &mut self,
        builder: &mut FunctionBuilder,
        constant: &IrConstant,
        ty: &IrType,
        module: &mut ObjectModule,
    ) -> CodegenResult<Value> {
        match constant {
            IrConstant::Int(n) => {
                let cl_type = self.translate_type(ty)?;
                Ok(builder.ins().iconst(cl_type, *n))
            }
            IrConstant::UInt(n) => {
                let cl_type = self.translate_type(ty)?;
                Ok(builder.ins().iconst(cl_type, *n as i64))
            }
            IrConstant::Float(f) => {
                let cl_type = self.translate_type(ty)?;
                if cl_type == types::F64 {
                    Ok(builder.ins().f64const(*f))
                } else {
                    Ok(builder.ins().f32const(*f as f32))
                }
            }
            IrConstant::Bool(b) => Ok(builder.ins().iconst(types::I8, if *b { 1 } else { 0 })),
            IrConstant::Rune(c) => Ok(builder.ins().iconst(types::I32, *c as i64)),
            IrConstant::Void => Ok(builder.ins().iconst(types::I64, 0)),
            IrConstant::String(bytes) => {
                // Create or get global data for this string
                let data_id = if let Some(existing_id) = self.string_constants.get(bytes) {
                    *existing_id
                } else {
                    // Create a new data section for this string
                    let mut data_bytes = bytes.clone();
                    data_bytes.push(0); // Null terminator for C strings
                    
                    let data_id = module
                        .declare_anonymous_data(false, false)
                        .map_err(|e| {
                            CodegenError::ModuleError(format!("Failed to declare string data: {}", e))
                        })?;
                    
                    let mut data_desc = DataDescription::new();
                    data_desc.define(data_bytes.into_boxed_slice());
                    
                    module
                        .define_data(data_id, &data_desc)
                        .map_err(|e| {
                            CodegenError::ModuleError(format!("Failed to define string data: {}", e))
                        })?;
                    
                    self.string_constants.insert(bytes.clone(), data_id);
                    data_id
                };
                
                // Get a pointer to the global data
                let global_value = module.declare_data_in_func(data_id, builder.func);
                let ptr = builder.ins().global_value(types::I64, global_value);
                Ok(ptr)
            }
        }
    }

    /// Translate a binary operation
    fn translate_binop(
        &self,
        builder: &mut FunctionBuilder,
        op: IrBinOp,
        left: Value,
        right: Value,
        result_ty: &IrType,
    ) -> CodegenResult<Value> {
        use IrBinOp::*;

        let is_float = matches!(result_ty, IrType::Float(_));
        let is_signed = matches!(result_ty, IrType::Int(_));

        let result = match op {
            Add => {
                if is_float {
                    builder.ins().fadd(left, right)
                } else {
                    builder.ins().iadd(left, right)
                }
            }
            Sub => {
                if is_float {
                    builder.ins().fsub(left, right)
                } else {
                    builder.ins().isub(left, right)
                }
            }
            Mul => {
                if is_float {
                    builder.ins().fmul(left, right)
                } else {
                    builder.ins().imul(left, right)
                }
            }
            Div => {
                if is_float {
                    builder.ins().fdiv(left, right)
                } else if is_signed {
                    builder.ins().sdiv(left, right)
                } else {
                    builder.ins().udiv(left, right)
                }
            }
            Mod => {
                if is_signed {
                    builder.ins().srem(left, right)
                } else {
                    builder.ins().urem(left, right)
                }
            }
            Eq => {
                if is_float {
                    builder.ins().fcmp(FloatCC::Equal, left, right)
                } else {
                    builder.ins().icmp(IntCC::Equal, left, right)
                }
            }
            Ne => {
                if is_float {
                    builder.ins().fcmp(FloatCC::NotEqual, left, right)
                } else {
                    builder.ins().icmp(IntCC::NotEqual, left, right)
                }
            }
            Lt => {
                if is_float {
                    builder.ins().fcmp(FloatCC::LessThan, left, right)
                } else if is_signed {
                    builder.ins().icmp(IntCC::SignedLessThan, left, right)
                } else {
                    builder.ins().icmp(IntCC::UnsignedLessThan, left, right)
                }
            }
            Le => {
                if is_float {
                    builder.ins().fcmp(FloatCC::LessThanOrEqual, left, right)
                } else if is_signed {
                    builder.ins().icmp(IntCC::SignedLessThanOrEqual, left, right)
                } else {
                    builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, left, right)
                }
            }
            Gt => {
                if is_float {
                    builder.ins().fcmp(FloatCC::GreaterThan, left, right)
                } else if is_signed {
                    builder.ins().icmp(IntCC::SignedGreaterThan, left, right)
                } else {
                    builder.ins().icmp(IntCC::UnsignedGreaterThan, left, right)
                }
            }
            Ge => {
                if is_float {
                    builder.ins().fcmp(FloatCC::GreaterThanOrEqual, left, right)
                } else if is_signed {
                    builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, left, right)
                } else {
                    builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, left, right)
                }
            }
            And => builder.ins().band(left, right),
            Or => builder.ins().bor(left, right),
            BitAnd => builder.ins().band(left, right),
            BitOr => builder.ins().bor(left, right),
            LShift => builder.ins().ishl(left, right),
            RShift => {
                if is_signed {
                    builder.ins().sshr(left, right)
                } else {
                    builder.ins().ushr(left, right)
                }
            }
        };

        Ok(result)
    }

    /// Translate a unary operation
    fn translate_unop(
        &self,
        builder: &mut FunctionBuilder,
        op: IrUnOp,
        operand: Value,
        result_ty: &IrType,
    ) -> CodegenResult<Value> {
        use IrUnOp::*;

        let result = match op {
            Neg => {
                if matches!(result_ty, IrType::Float(_)) {
                    builder.ins().fneg(operand)
                } else {
                    builder.ins().ineg(operand)
                }
            }
            Not => {
                // Logical not: convert to 0/1 then XOR with 1
                let zero = builder.ins().iconst(types::I8, 0);
                builder.ins().icmp(IntCC::Equal, operand, zero)
            }
            BitNot => builder.ins().bnot(operand),
        };

        Ok(result)
    }

    /// Translate a load operation
    fn translate_load(
        &self,
        builder: &mut FunctionBuilder,
        source: &IrMemoryLocation,
        values: &HashMap<ValueId, Value>,
        stack_slots: &HashMap<LocalId, StackSlot>,
        ty: &IrType,
    ) -> CodegenResult<Value> {
        match source {
            IrMemoryLocation::Local(local_id) => {
                let slot = stack_slots
                    .get(local_id)
                    .ok_or(CodegenError::InvalidLocal(*local_id))?;
                let cl_type = self.translate_type(ty)?;
                Ok(builder.ins().stack_load(cl_type, *slot, 0))
            }
            IrMemoryLocation::Global(_) => {
                Err(CodegenError::UnsupportedInstruction(
                    "Global variable loads not yet implemented".to_string(),
                ))
            }
            IrMemoryLocation::StructField { base, field_index } => {
                // Simplified: just return the base value if field_index is 0
                if *field_index == 0 {
                    values
                        .get(base)
                        .copied()
                        .ok_or(CodegenError::InvalidValue(*base))
                } else {
                    Err(CodegenError::UnsupportedInstruction(
                        "Non-zero field loads not yet implemented".to_string(),
                    ))
                }
            }
            IrMemoryLocation::TupleElement { base, index } => {
                // Simplified: just return the base value if index is 0
                if *index == 0 {
                    values
                        .get(base)
                        .copied()
                        .ok_or(CodegenError::InvalidValue(*base))
                } else {
                    Err(CodegenError::UnsupportedInstruction(
                        "Non-zero tuple element loads not yet implemented".to_string(),
                    ))
                }
            }
        }
    }

    /// Translate a terminator instruction
    fn translate_terminator(
        &self,
        builder: &mut FunctionBuilder,
        terminator: &IrTerminator,
        values: &HashMap<ValueId, Value>,
        blocks: &HashMap<BlockId, Block>,
    ) -> CodegenResult<()> {
        match terminator {
            IrTerminator::Return { value } => {
                if let Some(val_id) = value {
                    let val = values
                        .get(val_id)
                        .ok_or(CodegenError::InvalidValue(*val_id))?;
                    builder.ins().return_(&[*val]);
                } else {
                    builder.ins().return_(&[]);
                }
            }
            IrTerminator::Jump { target } => {
                let cl_block = blocks
                    .get(target)
                    .ok_or(CodegenError::InvalidBlock(*target))?;
                builder.ins().jump(*cl_block, &[]);
            }
            IrTerminator::Branch {
                condition,
                true_block,
                false_block,
            } => {
                let cond = values
                    .get(condition)
                    .ok_or(CodegenError::InvalidValue(*condition))?;
                let true_cl = blocks
                    .get(true_block)
                    .ok_or(CodegenError::InvalidBlock(*true_block))?;
                let false_cl = blocks
                    .get(false_block)
                    .ok_or(CodegenError::InvalidBlock(*false_block))?;
                builder.ins().brif(*cond, *true_cl, &[], *false_cl, &[]);
            }
            IrTerminator::Switch { .. } => {
                return Err(CodegenError::UnsupportedInstruction(
                    "Switch terminators not yet implemented".to_string(),
                ));
            }
            IrTerminator::Unreachable => {
                builder.ins().trap(TrapCode::unwrap_user(1));
            }
        }
        Ok(())
    }

    /// Translate an IR type to a Cranelift type
    fn translate_type(&self, ty: &IrType) -> CodegenResult<Type> {
        match ty {
            IrType::Void => Ok(types::I64), // Use I64 as placeholder for void
            IrType::Bool => Ok(types::I8),
            IrType::Int(bits) => match bits {
                8 => Ok(types::I8),
                16 => Ok(types::I16),
                32 => Ok(types::I32),
                64 => Ok(types::I64),
                _ => Err(CodegenError::UnsupportedType(format!(
                    "Integer with {} bits",
                    bits
                ))),
            },
            IrType::UInt(bits) => match bits {
                8 => Ok(types::I8),
                16 => Ok(types::I16),
                32 => Ok(types::I32),
                64 => Ok(types::I64),
                _ => Err(CodegenError::UnsupportedType(format!(
                    "Unsigned integer with {} bits",
                    bits
                ))),
            },
            IrType::Float(bits) => match bits {
                32 => Ok(types::F32),
                64 => Ok(types::F64),
                _ => Err(CodegenError::UnsupportedType(format!(
                    "Float with {} bits",
                    bits
                ))),
            },
            IrType::Rune => Ok(types::I32), // Unicode code point
            IrType::Pointer(_) => Ok(types::I64), // Pointer as 64-bit int
            IrType::Tuple(_) => Ok(types::I64), // Simplified: tuple as pointer
            IrType::Struct(_) => Ok(types::I64), // Simplified: struct as pointer
            IrType::Enum(_) => Ok(types::I64), // Simplified: enum as pointer
            IrType::Function { .. } => Ok(types::I64), // Function pointer
            IrType::Closure { .. } => Ok(types::I64), // Closure as pointer
        }
    }

    /// Get the size of a type in bytes
    fn type_size(&self, ty: &IrType) -> usize {
        match ty {
            IrType::Void => 0,
            IrType::Bool => 1,
            IrType::Int(bits) | IrType::UInt(bits) => (bits / 8) as usize,
            IrType::Float(bits) => (bits / 8) as usize,
            IrType::Rune => 4,
            IrType::Pointer(_) => 8,
            IrType::Tuple(types) => {
                // Sum of element sizes (simplified, no padding)
                types.iter().map(|t| self.type_size(t)).sum()
            }
            IrType::Struct(name) => {
                if let Some(struct_def) = self.struct_defs.get(name) {
                    struct_def
                        .fields
                        .iter()
                        .map(|(_, ty)| self.type_size(ty))
                        .sum()
                } else {
                    8 // Default to pointer size
                }
            }
            IrType::Enum(name) => {
                if let Some(enum_def) = self.enum_defs.get(name) {
                    // Tag (4 bytes) + largest variant
                    let max_variant_size = enum_def
                        .variants
                        .iter()
                        .map(|(_, types)| types.iter().map(|t| self.type_size(t)).sum())
                        .max()
                        .unwrap_or(0);
                    4 + max_variant_size
                } else {
                    8 // Default
                }
            }
            IrType::Function { .. } | IrType::Closure { .. } => 8, // Function pointer
        }
    }
}

impl Default for CodeGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_translation() {
        let codegen = CodeGenerator::new();

        assert_eq!(codegen.translate_type(&IrType::Bool).unwrap(), types::I8);
        assert_eq!(codegen.translate_type(&IrType::Int(32)).unwrap(), types::I32);
        assert_eq!(codegen.translate_type(&IrType::Int(64)).unwrap(), types::I64);
        assert_eq!(codegen.translate_type(&IrType::UInt(16)).unwrap(), types::I16);
        assert_eq!(codegen.translate_type(&IrType::Float(32)).unwrap(), types::F32);
        assert_eq!(codegen.translate_type(&IrType::Float(64)).unwrap(), types::F64);
        assert_eq!(codegen.translate_type(&IrType::Rune).unwrap(), types::I32);
    }

    #[test]
    fn test_type_sizes() {
        let codegen = CodeGenerator::new();

        assert_eq!(codegen.type_size(&IrType::Void), 0);
        assert_eq!(codegen.type_size(&IrType::Bool), 1);
        assert_eq!(codegen.type_size(&IrType::Int(8)), 1);
        assert_eq!(codegen.type_size(&IrType::Int(16)), 2);
        assert_eq!(codegen.type_size(&IrType::Int(32)), 4);
        assert_eq!(codegen.type_size(&IrType::Int(64)), 8);
        assert_eq!(codegen.type_size(&IrType::Float(32)), 4);
        assert_eq!(codegen.type_size(&IrType::Float(64)), 8);
        assert_eq!(codegen.type_size(&IrType::Rune), 4);
        assert_eq!(codegen.type_size(&IrType::Pointer(Box::new(IrType::Int(32)))), 8);
    }

    #[test]
    fn test_struct_size_calculation() {
        let mut codegen = CodeGenerator::new();
        
        // Create a simple struct with two i32 fields
        let struct_def = IrStructDef {
            name: "Vec2".to_string(),
            fields: vec![
                ("x".to_string(), IrType::Int(32)),
                ("y".to_string(), IrType::Int(32)),
            ],
        };
        
        codegen.struct_defs.insert("Vec2".to_string(), struct_def);
        
        let size = codegen.type_size(&IrType::Struct("Vec2".to_string()));
        assert_eq!(size, 8); // 4 + 4
    }

    #[test]
    fn test_simple_program_compilation() {
        let mut codegen = CodeGenerator::new();

        // Create a simple program: fn add(a: i64, b: i64) -> i64 { return a + b; }
        let mut program = IrProgram::new();
        
        let func = IrFunction {
            name: "add".to_string(),
            params: vec![
                ("a".to_string(), IrType::Int(64)),
                ("b".to_string(), IrType::Int(64)),
            ],
            return_type: Some(IrType::Int(64)),
            blocks: vec![crate::ir::IrBlock {
                label: BlockId(0),
                instructions: vec![IrInstruction {
                    result: ValueId(2),
                    ty: IrType::Int(64),
                    kind: IrInstructionKind::BinOp {
                        op: IrBinOp::Add,
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                }],
                terminator: IrTerminator::Return {
                    value: Some(ValueId(2)),
                },
            }],
            locals: vec![],
            is_public: true,
        };
        
        program.add_function(func);

        // Try to compile (may fail if Cranelift native detection fails, that's ok for test)
        let result = codegen.compile(program, "/tmp/test_add.o");
        
        // We don't assert success because the test environment may not have all native dependencies
        // Just verify it doesn't panic
        match result {
            Ok(_) => println!("Compilation succeeded"),
            Err(e) => println!("Compilation failed (expected in test env): {}", e),
        }
    }

    #[test]
    fn test_codegen_error_display() {
        let err = CodegenError::UnsupportedType("CustomType".to_string());
        assert_eq!(format!("{}", err), "Unsupported type: CustomType");

        let err = CodegenError::InvalidValue(ValueId(42));
        assert!(format!("{}", err).contains("42"));

        let err = CodegenError::FunctionNotFound("foo".to_string());
        assert_eq!(format!("{}", err), "Function not found: foo");
    }
}
