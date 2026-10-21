//! `include/pt-gpt-partnames.h`: the GPT partition types libfdisk knows,
//! generated from util-linux 2.39.3's header, in its order.

use crate::parttype::PartType;

/// `gpt_parttypes`.
pub(crate) static GPT_PARTTYPES: [PartType; 215] = [
    PartType::guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B", b"EFI System"),
    PartType::guid(
        b"024DEE41-33E7-11D3-9D69-0008C781F39F",
        b"MBR partition scheme",
    ),
    PartType::guid(b"D3BFE2DE-3DAF-11DF-BA40-E3A556D89593", b"Intel Fast Flash"),
    PartType::guid(b"21686148-6449-6E6F-744E-656564454649", b"BIOS boot"),
    PartType::guid(
        b"F4019732-066E-4E12-8273-346C5641494F",
        b"Sony boot partition",
    ),
    PartType::guid(
        b"BFBFAFE7-A34F-448A-9A5B-6213EB736C22",
        b"Lenovo boot partition",
    ),
    PartType::guid(
        b"9E1A2D38-C612-4316-AA26-8B49521E5A8B",
        b"PowerPC PReP boot",
    ),
    PartType::guid(b"7412F7D5-A156-4B13-81DC-867174929325", b"ONIE boot"),
    PartType::guid(b"D4E6E2CD-4469-46F3-B5CB-1BFF57AFC149", b"ONIE config"),
    PartType::guid(
        b"E3C9E316-0B5C-4DB8-817D-F92DF00215AE",
        b"Microsoft reserved",
    ),
    PartType::guid(
        b"EBD0A0A2-B9E5-4433-87C0-68B6B72699C7",
        b"Microsoft basic data",
    ),
    PartType::guid(
        b"5808C8AA-7E8F-42E0-85D2-E1E90434CFB3",
        b"Microsoft LDM metadata",
    ),
    PartType::guid(
        b"AF9B60A0-1431-4F62-BC68-3311714A69AD",
        b"Microsoft LDM data",
    ),
    PartType::guid(
        b"DE94BBA4-06D1-4D40-A16A-BFD50179D6AC",
        b"Windows recovery environment",
    ),
    PartType::guid(
        b"37AFFC90-EF7D-4E96-91C3-2D7AE055B174",
        b"IBM General Parallel Fs",
    ),
    PartType::guid(
        b"E75CAF8F-F680-4CEE-AFA3-B001E56EFC2D",
        b"Microsoft Storage Spaces",
    ),
    PartType::guid(b"75894C1E-3AEB-11D3-B7C1-7B03A0000000", b"HP-UX data"),
    PartType::guid(b"E2A1E728-32E3-11D6-A682-7B03A0000000", b"HP-UX service"),
    PartType::guid(b"0657FD6D-A4AB-43C4-84E5-0933C84B4F4F", b"Linux swap"),
    PartType::guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4", b"Linux filesystem"),
    PartType::guid(
        b"3B8F8425-20E0-4F3B-907F-1A25A76F98E8",
        b"Linux server data",
    ),
    PartType::guid(b"44479540-F297-41B2-9AF7-D131D5F0458A", b"Linux root (x86)"),
    PartType::guid(
        b"4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709",
        b"Linux root (x86-64)",
    ),
    PartType::guid(
        b"6523F8AE-3EB1-4E2A-A05A-18B695AE656F",
        b"Linux root (Alpha)",
    ),
    PartType::guid(b"D27F46ED-2919-4CB8-BD25-9531F3C16534", b"Linux root (ARC)"),
    PartType::guid(b"69DAD710-2CE4-4E3C-B16C-21A1D49ABED3", b"Linux root (ARM)"),
    PartType::guid(
        b"B921B045-1DF0-41C3-AF44-4C6F280D3FAE",
        b"Linux root (ARM-64)",
    ),
    PartType::guid(
        b"993D8D3D-F80E-4225-855A-9DAF8ED7EA97",
        b"Linux root (IA-64)",
    ),
    PartType::guid(
        b"77055800-792C-4F94-B39A-98C91B762BB6",
        b"Linux root (LoongArch-64)",
    ),
    PartType::guid(
        b"37C58C8A-D913-4156-A25F-48B1B64E07F0",
        b"Linux root (MIPS-32 LE)",
    ),
    PartType::guid(
        b"700BDA43-7A34-4507-B179-EEB93D7A7CA3",
        b"Linux root (MIPS-64 LE)",
    ),
    PartType::guid(
        b"1AACDB3B-5444-4138-BD9E-E5C2239B2346",
        b"Linux root (HPPA/PARISC)",
    ),
    PartType::guid(b"1DE3F1EF-FA98-47B5-8DCD-4A860A654D78", b"Linux root (PPC)"),
    PartType::guid(
        b"912ADE1D-A839-4913-8964-A10EEE08FBD2",
        b"Linux root (PPC64)",
    ),
    PartType::guid(
        b"C31C45E6-3F39-412E-80FB-4809C4980599",
        b"Linux root (PPC64LE)",
    ),
    PartType::guid(
        b"60D5A7FE-8E7D-435C-B714-3DD8162144E1",
        b"Linux root (RISC-V-32)",
    ),
    PartType::guid(
        b"72EC70A6-CF74-40E6-BD49-4BDA08E8F224",
        b"Linux root (RISC-V-64)",
    ),
    PartType::guid(
        b"08A7ACEA-624C-4A20-91E8-6E0FA67D23F9",
        b"Linux root (S390)",
    ),
    PartType::guid(
        b"5EEAD9A9-FE09-4A1E-A1D7-520D00531306",
        b"Linux root (S390X)",
    ),
    PartType::guid(
        b"C50CDD70-3862-4CC3-90E1-809A8C93EE2C",
        b"Linux root (TILE-Gx)",
    ),
    PartType::guid(b"8DA63339-0007-60C0-C436-083AC8230908", b"Linux reserved"),
    PartType::guid(b"933AC7E1-2EB4-4F13-B844-0E14E2AEF915", b"Linux home"),
    PartType::guid(b"A19D880F-05FC-4D3B-A006-743F0F84911E", b"Linux RAID"),
    PartType::guid(b"E6D6D379-F507-44C2-A23C-238F2A3DF928", b"Linux LVM"),
    PartType::guid(
        b"4D21B016-B534-45C2-A9FB-5C16E091FD2D",
        b"Linux variable data",
    ),
    PartType::guid(
        b"7EC6F557-3BC5-4ACA-B293-16EF5DF639D1",
        b"Linux temporary data",
    ),
    PartType::guid(b"75250D76-8CC6-458E-BD66-BD47CC81A812", b"Linux /usr (x86)"),
    PartType::guid(
        b"8484680C-9521-48C6-9C11-B0720656F69E",
        b"Linux /usr (x86-64)",
    ),
    PartType::guid(
        b"E18CF08C-33EC-4C0D-8246-C6C6FB3DA024",
        b"Linux /usr (Alpha)",
    ),
    PartType::guid(b"7978A683-6316-4922-BBEE-38BFF5A2FECC", b"Linux /usr (ARC)"),
    PartType::guid(b"7D0359A3-02B3-4F0A-865C-654403E70625", b"Linux /usr (ARM)"),
    PartType::guid(
        b"B0E01050-EE5F-4390-949A-9101B17104E9",
        b"Linux /usr (ARM-64)",
    ),
    PartType::guid(
        b"4301D2A6-4E3B-4B2A-BB94-9E0B2C4225EA",
        b"Linux /usr (IA-64)",
    ),
    PartType::guid(
        b"E611C702-575C-4CBE-9A46-434FA0BF7E3F",
        b"Linux /usr (LoongArch-64)",
    ),
    PartType::guid(
        b"0F4868E9-9952-4706-979F-3ED3A473E947",
        b"Linux /usr (MIPS-32 LE)",
    ),
    PartType::guid(
        b"C97C1F32-BA06-40B4-9F22-236061B08AA8",
        b"Linux /usr (MIPS-64 LE)",
    ),
    PartType::guid(
        b"DC4A4480-6917-4262-A4EC-DB9384949F25",
        b"Linux /usr (HPPA/PARISC)",
    ),
    PartType::guid(b"7D14FEC5-CC71-415D-9D6C-06BF0B3C3EAF", b"Linux /usr (PPC)"),
    PartType::guid(
        b"2C9739E2-F068-46B3-9FD0-01C5A9AFBCCA",
        b"Linux /usr (PPC64)",
    ),
    PartType::guid(
        b"15BB03AF-77E7-4D4A-B12B-C0D084F7491C",
        b"Linux /usr (PPC64LE)",
    ),
    PartType::guid(
        b"B933FB22-5C3F-4F91-AF90-E2BB0FA50702",
        b"Linux /usr (RISC-V-32)",
    ),
    PartType::guid(
        b"BEAEC34B-8442-439B-A40B-984381ED097D",
        b"Linux /usr (RISC-V-64)",
    ),
    PartType::guid(
        b"CD0F869B-D0FB-4CA0-B141-9EA87CC78D66",
        b"Linux /usr (S390)",
    ),
    PartType::guid(
        b"8A4F5770-50AA-4ED3-874A-99B710DB6FEA",
        b"Linux /usr (S390X)",
    ),
    PartType::guid(
        b"55497029-C7C1-44CC-AA39-815ED1558630",
        b"Linux /usr (TILE-Gx)",
    ),
    PartType::guid(
        b"D13C5D3B-B5D1-422A-B29F-9454FDC89D76",
        b"Linux root verity (x86)",
    ),
    PartType::guid(
        b"2C7357ED-EBD2-46D9-AEC1-23D437EC2BF5",
        b"Linux root verity (x86-64)",
    ),
    PartType::guid(
        b"FC56D9E9-E6E5-4C06-BE32-E74407CE09A5",
        b"Linux root verity (Alpha)",
    ),
    PartType::guid(
        b"24B2D975-0F97-4521-AFA1-CD531E421B8D",
        b"Linux root verity (ARC)",
    ),
    PartType::guid(
        b"7386CDF2-203C-47A9-A498-F2ECCE45A2D6",
        b"Linux root verity (ARM)",
    ),
    PartType::guid(
        b"DF3300CE-D69F-4C92-978C-9BFB0F38D820",
        b"Linux root verity (ARM-64)",
    ),
    PartType::guid(
        b"86ED10D5-B607-45BB-8957-D350F23D0571",
        b"Linux root verity (IA-64)",
    ),
    PartType::guid(
        b"F3393B22-E9AF-4613-A948-9D3BFBD0C535",
        b"Linux root verity (LoongArch-64)",
    ),
    PartType::guid(
        b"D7D150D2-2A04-4A33-8F12-16651205FF7B",
        b"Linux root verity (MIPS-32 LE)",
    ),
    PartType::guid(
        b"16B417F8-3E06-4F57-8DD2-9B5232F41AA6",
        b"Linux root verity (MIPS-64 LE)",
    ),
    PartType::guid(
        b"D212A430-FBC5-49F9-A983-A7FEEF2B8D0E",
        b"Linux root verity (HPPA/PARISC)",
    ),
    PartType::guid(
        b"98CFE649-1588-46DC-B2F0-ADD147424925",
        b"Linux root verity (PPC)",
    ),
    PartType::guid(
        b"9225A9A3-3C19-4D89-B4F6-EEFF88F17631",
        b"Linux root verity (PPC64)",
    ),
    PartType::guid(
        b"906BD944-4589-4AAE-A4E4-DD983917446A",
        b"Linux root verity (PPC64LE)",
    ),
    PartType::guid(
        b"AE0253BE-1167-4007-AC68-43926C14C5DE",
        b"Linux root verity (RISC-V-32)",
    ),
    PartType::guid(
        b"B6ED5582-440B-4209-B8DA-5FF7C419EA3D",
        b"Linux root verity (RISC-V-64)",
    ),
    PartType::guid(
        b"7AC63B47-B25C-463B-8DF8-B4A94E6C90E1",
        b"Linux root verity (S390)",
    ),
    PartType::guid(
        b"B325BFBE-C7BE-4AB8-8357-139E652D2F6B",
        b"Linux root verity (S390X)",
    ),
    PartType::guid(
        b"966061EC-28E4-4B2E-B4A5-1F0A825A1D84",
        b"Linux root verity (TILE-Gx)",
    ),
    PartType::guid(
        b"8F461B0D-14EE-4E81-9AA9-049B6FB97ABD",
        b"Linux /usr verity (x86)",
    ),
    PartType::guid(
        b"77FF5F63-E7B6-4633-ACF4-1565B864C0E6",
        b"Linux /usr verity (x86-64)",
    ),
    PartType::guid(
        b"8CCE0D25-C0D0-4A44-BD87-46331BF1DF67",
        b"Linux /usr verity (Alpha)",
    ),
    PartType::guid(
        b"FCA0598C-D880-4591-8C16-4EDA05C7347C",
        b"Linux /usr verity (ARC)",
    ),
    PartType::guid(
        b"C215D751-7BCD-4649-BE90-6627490A4C05",
        b"Linux /usr verity (ARM)",
    ),
    PartType::guid(
        b"6E11A4E7-FBCA-4DED-B9E9-E1A512BB664E",
        b"Linux /usr verity (ARM-64)",
    ),
    PartType::guid(
        b"6A491E03-3BE7-4545-8E38-83320E0EA880",
        b"Linux /usr verity (IA-64)",
    ),
    PartType::guid(
        b"F46B2C26-59AE-48F0-9106-C50ED47F673D",
        b"Linux /usr verity (LoongArch-64)",
    ),
    PartType::guid(
        b"46B98D8D-B55C-4E8F-AAB3-37FCA7F80752",
        b"Linux /usr verity (MIPS-32 LE)",
    ),
    PartType::guid(
        b"3C3D61FE-B5F3-414D-BB71-8739A694A4EF",
        b"Linux /usr verity (MIPS-64 LE)",
    ),
    PartType::guid(
        b"5843D618-EC37-48D7-9F12-CEA8E08768B2",
        b"Linux /usr verity (HPPA/PARISC)",
    ),
    PartType::guid(
        b"DF765D00-270E-49E5-BC75-F47BB2118B09",
        b"Linux /usr verity (PPC)",
    ),
    PartType::guid(
        b"BDB528A5-A259-475F-A87D-DA53FA736A07",
        b"Linux /usr verity (PPC64)",
    ),
    PartType::guid(
        b"EE2B9983-21E8-4153-86D9-B6901A54D1CE",
        b"Linux /usr verity (PPC64LE)",
    ),
    PartType::guid(
        b"CB1EE4E3-8CD0-4136-A0A4-AA61A32E8730",
        b"Linux /usr verity (RISC-V-32)",
    ),
    PartType::guid(
        b"8F1056BE-9B05-47C4-81D6-BE53128E5B54",
        b"Linux /usr verity (RISC-V-64)",
    ),
    PartType::guid(
        b"B663C618-E7BC-4D6D-90AA-11B756BB1797",
        b"Linux /usr verity (S390)",
    ),
    PartType::guid(
        b"31741CC4-1A2A-4111-A581-E00B447D2D06",
        b"Linux /usr verity (S390X)",
    ),
    PartType::guid(
        b"2FB4BF56-07FA-42DA-8132-6B139F2026AE",
        b"Linux /usr verity (TILE-Gx)",
    ),
    PartType::guid(
        b"5996FC05-109C-48DE-808B-23FA0830B676",
        b"Linux root verity sign. (x86)",
    ),
    PartType::guid(
        b"41092B05-9FC8-4523-994F-2DEF0408B176",
        b"Linux root verity sign. (x86-64)",
    ),
    PartType::guid(
        b"D46495B7-A053-414F-80F7-700C99921EF8",
        b"Linux root verity sign. (Alpha)",
    ),
    PartType::guid(
        b"143A70BA-CBD3-4F06-919F-6C05683A78BC",
        b"Linux root verity sign. (ARC)",
    ),
    PartType::guid(
        b"42B0455F-EB11-491D-98D3-56145BA9D037",
        b"Linux root verity sign. (ARM)",
    ),
    PartType::guid(
        b"6DB69DE6-29F4-4758-A7A5-962190F00CE3",
        b"Linux root verity sign. (ARM-64)",
    ),
    PartType::guid(
        b"E98B36EE-32BA-4882-9B12-0CE14655F46A",
        b"Linux root verity sign. (IA-64)",
    ),
    PartType::guid(
        b"5AFB67EB-ECC8-4F85-AE8E-AC1E7C50E7D0",
        b"Linux root verity sign. (LoongArch-64)",
    ),
    PartType::guid(
        b"C919CC1F-4456-4EFF-918C-F75E94525CA5",
        b"Linux root verity sign. (MIPS-32 LE)",
    ),
    PartType::guid(
        b"904E58EF-5C65-4A31-9C57-6AF5FC7C5DE7",
        b"Linux root verity sign. (MIPS-64 LE)",
    ),
    PartType::guid(
        b"15DE6170-65D3-431C-916E-B0DCD8393F25",
        b"Linux root verity sign. (HPPA/PARISC)",
    ),
    PartType::guid(
        b"1B31B5AA-ADD9-463A-B2ED-BD467FC857E7",
        b"Linux root verity sign. (PPC)",
    ),
    PartType::guid(
        b"F5E2C20C-45B2-4FFA-BCE9-2A60737E1AAF",
        b"Linux root verity sign. (PPC64)",
    ),
    PartType::guid(
        b"D4A236E7-E873-4C07-BF1D-BF6CF7F1C3C6",
        b"Linux root verity sign. (PPC64LE)",
    ),
    PartType::guid(
        b"3A112A75-8729-4380-B4CF-764D79934448",
        b"Linux root verity sign. (RISC-V-32)",
    ),
    PartType::guid(
        b"EFE0F087-EA8D-4469-821A-4C2A96A8386A",
        b"Linux root verity sign. (RISC-V-64)",
    ),
    PartType::guid(
        b"3482388E-4254-435A-A241-766A065F9960",
        b"Linux root verity sign. (S390)",
    ),
    PartType::guid(
        b"C80187A5-73A3-491A-901A-017C3FA953E9",
        b"Linux root verity sign. (S390X)",
    ),
    PartType::guid(
        b"B3671439-97B0-4A53-90F7-2D5A8F3AD47B",
        b"Linux root verity sign. (TILE-Gx)",
    ),
    PartType::guid(
        b"974A71C0-DE41-43C3-BE5D-5C5CCD1AD2C0",
        b"Linux /usr verity sign. (x86)",
    ),
    PartType::guid(
        b"E7BB33FB-06CF-4E81-8273-E543B413E2E2",
        b"Linux /usr verity sign. (x86-64)",
    ),
    PartType::guid(
        b"5C6E1C76-076A-457A-A0FE-F3B4CD21CE6E",
        b"Linux /usr verity sign. (Alpha)",
    ),
    PartType::guid(
        b"94F9A9A1-9971-427A-A400-50CB297F0F35",
        b"Linux /usr verity sign. (ARC)",
    ),
    PartType::guid(
        b"D7FF812F-37D1-4902-A810-D76BA57B975A",
        b"Linux /usr verity sign. (ARM)",
    ),
    PartType::guid(
        b"C23CE4FF-44BD-4B00-B2D4-B41B3419E02A",
        b"Linux /usr verity sign. (ARM-64)",
    ),
    PartType::guid(
        b"8DE58BC2-2A43-460D-B14E-A76E4A17B47F",
        b"Linux /usr verity sign. (IA-64)",
    ),
    PartType::guid(
        b"B024F315-D330-444C-8461-44BBDE524E99",
        b"Linux /usr verity sign. (LoongArch-64)",
    ),
    PartType::guid(
        b"3E23CA0B-A4BC-4B4E-8087-5AB6A26AA8A9",
        b"Linux /usr verity sign. (MIPS-32 LE)",
    ),
    PartType::guid(
        b"F2C2C7EE-ADCC-4351-B5C6-EE9816B66E16",
        b"Linux /usr verity sign. (MIPS-64 LE)",
    ),
    PartType::guid(
        b"450DD7D1-3224-45EC-9CF2-A43A346D71EE",
        b"Linux /usr verity sign. (HPPA/PARISC)",
    ),
    PartType::guid(
        b"7007891D-D371-4A80-86A4-5CB875B9302E",
        b"Linux /usr verity sign. (PPC)",
    ),
    PartType::guid(
        b"0B888863-D7F8-4D9E-9766-239FCE4D58AF",
        b"Linux /usr verity sign. (PPC64)",
    ),
    PartType::guid(
        b"C8BFBD1E-268E-4521-8BBA-BF314C399557",
        b"Linux /usr verity sign. (PPC64LE)",
    ),
    PartType::guid(
        b"C3836A13-3137-45BA-B583-B16C50FE5EB4",
        b"Linux /usr verity sign. (RISC-V-32)",
    ),
    PartType::guid(
        b"D2F9000A-7A18-453F-B5CD-4D32F77A7B32",
        b"Linux /usr verity sign. (RISC-V-64)",
    ),
    PartType::guid(
        b"17440E4F-A8D0-467F-A46E-3912AE6EF2C5",
        b"Linux /usr verity sign. (S390)",
    ),
    PartType::guid(
        b"3F324816-667B-46AE-86EE-9B0C0C6C11B4",
        b"Linux /usr verity sign. (S390X)",
    ),
    PartType::guid(
        b"4EDE75E2-6CCC-4CC8-B9C7-70334B087510",
        b"Linux /usr verity sign. (TILE-Gx)",
    ),
    PartType::guid(
        b"BC13C2FF-59E6-4262-A352-B275FD6F7172",
        b"Linux extended boot",
    ),
    PartType::guid(
        b"773f91ef-66d4-49b5-bd83-d683bf40ad16",
        b"Linux user's home",
    ),
    PartType::guid(b"516E7CB4-6ECF-11D6-8FF8-00022D09712B", b"FreeBSD data"),
    PartType::guid(b"83BD6B9D-7F41-11DC-BE0B-001560B84F0F", b"FreeBSD boot"),
    PartType::guid(b"516E7CB5-6ECF-11D6-8FF8-00022D09712B", b"FreeBSD swap"),
    PartType::guid(b"516E7CB6-6ECF-11D6-8FF8-00022D09712B", b"FreeBSD UFS"),
    PartType::guid(b"516E7CBA-6ECF-11D6-8FF8-00022D09712B", b"FreeBSD ZFS"),
    PartType::guid(b"516E7CB8-6ECF-11D6-8FF8-00022D09712B", b"FreeBSD Vinum"),
    PartType::guid(b"48465300-0000-11AA-AA11-00306543ECAC", b"Apple HFS/HFS+"),
    PartType::guid(b"7C3457EF-0000-11AA-AA11-00306543ECAC", b"Apple APFS"),
    PartType::guid(b"55465300-0000-11AA-AA11-00306543ECAC", b"Apple UFS"),
    PartType::guid(b"52414944-0000-11AA-AA11-00306543ECAC", b"Apple RAID"),
    PartType::guid(
        b"52414944-5F4F-11AA-AA11-00306543ECAC",
        b"Apple RAID offline",
    ),
    PartType::guid(b"426F6F74-0000-11AA-AA11-00306543ECAC", b"Apple boot"),
    PartType::guid(b"4C616265-6C00-11AA-AA11-00306543ECAC", b"Apple label"),
    PartType::guid(
        b"5265636F-7665-11AA-AA11-00306543ECAC",
        b"Apple TV recovery",
    ),
    PartType::guid(
        b"53746F72-6167-11AA-AA11-00306543ECAC",
        b"Apple Core storage",
    ),
    PartType::guid(
        b"69646961-6700-11AA-AA11-00306543ECAC",
        b"Apple Silicon boot",
    ),
    PartType::guid(
        b"52637672-7900-11AA-AA11-00306543ECAC",
        b"Apple Silicon recovery",
    ),
    PartType::guid(b"6A82CB45-1DD2-11B2-99A6-080020736631", b"Solaris boot"),
    PartType::guid(b"6A85CF4D-1DD2-11B2-99A6-080020736631", b"Solaris root"),
    PartType::guid(
        b"6A898CC3-1DD2-11B2-99A6-080020736631",
        b"Solaris /usr & Apple ZFS",
    ),
    PartType::guid(b"6A87C46F-1DD2-11B2-99A6-080020736631", b"Solaris swap"),
    PartType::guid(b"6A8B642B-1DD2-11B2-99A6-080020736631", b"Solaris backup"),
    PartType::guid(b"6A8EF2E9-1DD2-11B2-99A6-080020736631", b"Solaris /var"),
    PartType::guid(b"6A90BA39-1DD2-11B2-99A6-080020736631", b"Solaris /home"),
    PartType::guid(
        b"6A9283A5-1DD2-11B2-99A6-080020736631",
        b"Solaris alternate sector",
    ),
    PartType::guid(
        b"6A945A3B-1DD2-11B2-99A6-080020736631",
        b"Solaris reserved 1",
    ),
    PartType::guid(
        b"6A9630D1-1DD2-11B2-99A6-080020736631",
        b"Solaris reserved 2",
    ),
    PartType::guid(
        b"6A980767-1DD2-11B2-99A6-080020736631",
        b"Solaris reserved 3",
    ),
    PartType::guid(
        b"6A96237F-1DD2-11B2-99A6-080020736631",
        b"Solaris reserved 4",
    ),
    PartType::guid(
        b"6A8D2AC7-1DD2-11B2-99A6-080020736631",
        b"Solaris reserved 5",
    ),
    PartType::guid(b"49F48D32-B10E-11DC-B99B-0019D1879648", b"NetBSD swap"),
    PartType::guid(b"49F48D5A-B10E-11DC-B99B-0019D1879648", b"NetBSD FFS"),
    PartType::guid(b"49F48D82-B10E-11DC-B99B-0019D1879648", b"NetBSD LFS"),
    PartType::guid(
        b"2DB519C4-B10F-11DC-B99B-0019D1879648",
        b"NetBSD concatenated",
    ),
    PartType::guid(b"2DB519EC-B10F-11DC-B99B-0019D1879648", b"NetBSD encrypted"),
    PartType::guid(b"49F48DAA-B10E-11DC-B99B-0019D1879648", b"NetBSD RAID"),
    PartType::guid(b"FE3A2A5D-4F32-41A7-B725-ACCC3285A309", b"ChromeOS kernel"),
    PartType::guid(b"3CB8E202-3B7E-47DD-8A3C-7FF2A13CFCEC", b"ChromeOS root fs"),
    PartType::guid(
        b"2E0A753D-9E48-43B0-8337-B15192CB1B5E",
        b"ChromeOS reserved",
    ),
    PartType::guid(b"85D5E45A-237C-11E1-B4B3-E89A8F7FC3A7", b"MidnightBSD data"),
    PartType::guid(b"85D5E45E-237C-11E1-B4B3-E89A8F7FC3A7", b"MidnightBSD boot"),
    PartType::guid(b"85D5E45B-237C-11E1-B4B3-E89A8F7FC3A7", b"MidnightBSD swap"),
    PartType::guid(b"0394EF8B-237E-11E1-B4B3-E89A8F7FC3A7", b"MidnightBSD UFS"),
    PartType::guid(b"85D5E45D-237C-11E1-B4B3-E89A8F7FC3A7", b"MidnightBSD ZFS"),
    PartType::guid(
        b"85D5E45C-237C-11E1-B4B3-E89A8F7FC3A7",
        b"MidnightBSD Vinum",
    ),
    PartType::guid(b"45B0969E-9B03-4F30-B4C6-B4B80CEFF106", b"Ceph Journal"),
    PartType::guid(
        b"45B0969E-9B03-4F30-B4C6-5EC00CEFF106",
        b"Ceph Encrypted Journal",
    ),
    PartType::guid(b"4FBD7E29-9D25-41B8-AFD0-062C0CEFF05D", b"Ceph OSD"),
    PartType::guid(b"4FBD7E29-9D25-41B8-AFD0-5EC00CEFF05D", b"Ceph crypt OSD"),
    PartType::guid(
        b"89C57F98-2FE5-4DC0-89C1-F3AD0CEFF2BE",
        b"Ceph disk in creation",
    ),
    PartType::guid(
        b"89C57F98-2FE5-4DC0-89C1-5EC00CEFF2BE",
        b"Ceph crypt disk in creation",
    ),
    PartType::guid(b"AA31E02A-400F-11DB-9590-000C2911D1B8", b"VMware VMFS"),
    PartType::guid(
        b"9D275380-40AD-11DB-BF97-000C2911D1B8",
        b"VMware Diagnostic",
    ),
    PartType::guid(
        b"381CFCCC-7288-11E0-92EE-000C2911D0B2",
        b"VMware Virtual SAN",
    ),
    PartType::guid(b"77719A0C-A4A0-11E3-A47E-000C29745A24", b"VMware Virsto"),
    PartType::guid(b"9198EFFC-31C0-11DB-8F78-000C2911D1B8", b"VMware Reserved"),
    PartType::guid(b"824CC7A0-36A8-11E3-890A-952519AD3F61", b"OpenBSD data"),
    PartType::guid(b"CEF5A9AD-73BC-4601-89F3-CDEEEEE321A1", b"QNX6 file system"),
    PartType::guid(b"C91818F9-8025-47AF-89D2-F030D7000C2C", b"Plan 9 partition"),
    PartType::guid(b"5B193300-FC78-40CD-8002-E86C45580B47", b"HiFive FSBL"),
    PartType::guid(b"2E54B353-1271-4842-806F-E436D6AF6985", b"HiFive BBL"),
    PartType::guid(b"42465331-3BA3-10F1-802A-4861696B7521", b"Haiku BFS"),
    PartType::guid(
        b"6828311A-BA55-42A4-BCDE-A89BB5EDECAE",
        b"Marvell Armada 3700 Boot partition",
    ),
    PartType::guid(
        b"9D087404-1CA5-11DC-8817-01301BB8A9F5",
        b"DragonFlyBSD Label32",
    ),
    PartType::guid(
        b"9D58FDBD-1CA5-11DC-8817-01301BB8A9F5",
        b"DragonFlyBSD Swap",
    ),
    PartType::guid(
        b"9D94CE7C-1CA5-11DC-8817-01301BB8A9F5",
        b"DragonFlyBSD UFS1",
    ),
    PartType::guid(
        b"9DD4478F-1CA5-11DC-8817-01301BB8A9F5",
        b"DragonFlyBSD Vinum",
    ),
    PartType::guid(b"DBD5211B-1CA5-11DC-8817-01301BB8A9F5", b"DragonFlyBSD CCD"),
    PartType::guid(
        b"3D48CE54-1D16-11DC-8696-01301BB8A9F5",
        b"DragonFlyBSD Label64",
    ),
    PartType::guid(
        b"BD215AB2-1D16-11DC-8696-01301BB8A9F5",
        b"DragonFlyBSD Legacy",
    ),
    PartType::guid(
        b"61DC63AC-6E38-11DC-8513-01301BB8A9F5",
        b"DragonFlyBSD HAMMER",
    ),
    PartType::guid(
        b"5CBB9AD1-862D-11DC-A94D-01301BB8A9F5",
        b"DragonFlyBSD HAMMER2",
    ),
];
