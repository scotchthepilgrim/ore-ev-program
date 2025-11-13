use pinocchio::program_error::ProgramError;

pub mod ore_deploy;

pub use ore_deploy::*;

#[repr(u8)]
pub enum MyProgramInstruction {
    OreDeploy = 6,
}

impl TryFrom<&u8> for MyProgramInstruction {
    type Error = ProgramError;

    fn try_from(value: &u8) -> Result<Self, Self::Error> {
        match *value {
            1 => Ok(MyProgramInstruction::OreDeploy),
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }
}
