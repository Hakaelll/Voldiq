use super::{
    COPY_LIMIT, Discovery, ENUMERATION_WINDOW_MS, EnumerationCoverage, ShadowCopy, Unavailable,
};
use crate::{NodeId, index::Index, space::VolumeIdentity, windows as filesystem};
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{E_ACCESSDENIED, REGDB_E_CLASSNOTREG},
        System::{
            Com::*,
            Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE},
            Variant::*,
            Wmi::*,
        },
    },
    core::{BSTR, GUID, IUnknown, Interface, PCWSTR, w},
};

type Read = (Option<VolumeIdentity>, Discovery);
fn unavailable(reason: Unavailable) -> Read {
    (None, Discovery::Unavailable { reason })
}

pub(super) fn read(index: &Index, cancelled: &AtomicBool) -> Option<Read> {
    let Ok(pin) = index.pin_folder(NodeId(0)) else {
        return Some(unavailable(Unavailable::RootUnavailable));
    };
    let bound = (|| {
        let file = filesystem::open_metadata(pin.path())?;
        let metadata = file.metadata()?;
        if filesystem::is_reparse(&metadata)
            || filesystem::requires_recall(&metadata)
            || filesystem::file_identity(&file)?.key != pin.identity()
        {
            return Err(crate::Error::Invalid("Recovery root changed.".into()));
        }
        let (_, root) = filesystem::volume_paths(&file)?;
        Ok((
            file,
            VolumeIdentity {
                guid_root: root.to_string_lossy().into_owned(),
                serial: format!("{:016x}", pin.identity().0),
            },
        ))
    })();
    let Ok((file, volume)) = bound else {
        return Some(unavailable(Unavailable::RootUnavailable));
    };
    let shadows = match discover(&volume, cancelled) {
        Ok(Some(discovery)) => discovery,
        Ok(None) => return None,
        Err(reason) => Discovery::Unavailable { reason },
    };
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let valid = (|| {
        let current = index.pin_folder(NodeId(0))?;
        Ok::<_, crate::Error>(
            current.identity() == pin.identity()
                && current.ancestor_identities()? == pin.ancestor_identities()?
                && filesystem::file_identity(&file)?.key == pin.identity(),
        )
    })();
    if !matches!(valid, Ok(true)) {
        return Some(unavailable(Unavailable::RootUnavailable));
    }
    Some((Some(volume), shadows))
}

struct Apartment(PhantomData<Rc<()>>);
impl Apartment {
    fn new() -> Result<Self, Unavailable> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(failure)?;
        Ok(Self(PhantomData))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn failure(error: windows::core::Error) -> Unavailable {
    let code = error.code();
    if code == E_ACCESSDENIED || code.0 == WBEM_E_ACCESS_DENIED.0 {
        Unavailable::AccessDenied
    } else if code == REGDB_E_CLASSNOTREG
        || [
            WBEM_E_INVALID_CLASS,
            WBEM_E_INVALID_NAMESPACE,
            WBEM_E_NOT_SUPPORTED,
            WBEM_E_PROVIDER_NOT_FOUND,
            WBEM_E_PROVIDER_LOAD_FAILURE,
        ]
        .iter()
        .any(|value| value.0 == code.0)
    {
        Unavailable::ProviderUnavailable
    } else {
        Unavailable::NativeFailure
    }
}

fn secure<T: Interface>(proxy: &T) -> Result<(), Unavailable> {
    let unknown: IUnknown = proxy.cast().map_err(failure)?;
    unsafe {
        CoSetProxyBlanket(
            &unknown,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_PKT_PRIVACY,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
    .map_err(failure)
}

struct Property(VARIANT);
impl Drop for Property {
    fn drop(&mut self) {
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}
impl Property {
    fn get(object: &IWbemClassObject, name: PCWSTR) -> Result<Self, Unavailable> {
        let mut value = Self(VARIANT::default());
        unsafe { object.Get(name, 0, &mut value.0, None, None) }.map_err(failure)?;
        Ok(value)
    }
    fn text(&self) -> Result<Option<String>, Unavailable> {
        let value = unsafe { &self.0.Anonymous.Anonymous };
        match value.vt {
            VT_EMPTY | VT_NULL => Ok(None),
            VT_BSTR => {
                let text = unsafe { &value.Anonymous.bstrVal };
                if text.len() > 512 {
                    return Err(Unavailable::InvalidMetadata);
                }
                String::from_utf16(text)
                    .map(Some)
                    .map_err(|_| Unavailable::InvalidMetadata)
            }
            _ => Err(Unavailable::InvalidMetadata),
        }
    }
    fn number(&self) -> Result<Option<u32>, Unavailable> {
        let value = unsafe { &self.0.Anonymous.Anonymous };
        match value.vt {
            VT_EMPTY | VT_NULL => Ok(None),
            VT_I4 => u32::try_from(unsafe { value.Anonymous.lVal })
                .map(Some)
                .map_err(|_| Unavailable::InvalidMetadata),
            VT_UI4 => Ok(Some(unsafe { value.Anonymous.ulVal })),
            _ => Err(Unavailable::InvalidMetadata),
        }
    }
    fn boolean(&self) -> Result<Option<bool>, Unavailable> {
        let value = unsafe { &self.0.Anonymous.Anonymous };
        match value.vt {
            VT_EMPTY | VT_NULL => Ok(None),
            VT_BOOL => match unsafe { value.Anonymous.boolVal.0 } {
                0 => Ok(Some(false)),
                -1 => Ok(Some(true)),
                _ => Err(Unavailable::InvalidMetadata),
            },
            _ => Err(Unavailable::InvalidMetadata),
        }
    }
}

fn guid(value: &str) -> Option<String> {
    let body = if let Some(value) = value.strip_prefix('{') {
        value.strip_suffix('}')?
    } else {
        value
    };
    GUID::try_from(body).ok()?;
    Some(format!("{{{}}}", body.to_ascii_lowercase()))
}

fn copy(object: &IWbemClassObject, volume: &VolumeIdentity) -> Result<ShadowCopy, Unavailable> {
    let original = Property::get(object, w!("VolumeName"))?
        .text()?
        .ok_or(Unavailable::InvalidMetadata)?;
    if !original.eq_ignore_ascii_case(&volume.guid_root)
        || Property::get(object, w!("State"))?.number()? != Some(9)
    {
        return Err(Unavailable::InvalidMetadata);
    }
    let id = Property::get(object, w!("ID"))?
        .text()?
        .and_then(|value| guid(&value))
        .ok_or(Unavailable::InvalidMetadata)?;
    Ok(ShadowCopy {
        id,
        provider_id: Property::get(object, w!("ProviderID"))?
            .text()?
            .and_then(|value| guid(&value)),
        installed_at: Property::get(object, w!("InstallDate"))?
            .text()?
            .and_then(|value| super::cim_datetime(&value)),
        previous_versions: Property::get(object, w!("ClientAccessible"))?.boolean()?,
        persistent: Property::get(object, w!("Persistent"))?.boolean()?,
    })
}

fn discover(
    volume: &VolumeIdentity,
    cancelled: &AtomicBool,
) -> Result<Option<Discovery>, Unavailable> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let _apartment = Apartment::new()?;
    let locator: IWbemLocator =
        unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }.map_err(failure)?;
    let services = unsafe {
        locator.ConnectServer(
            &BSTR::from(r"ROOT\CIMV2"),
            &BSTR::new(),
            &BSTR::new(),
            &BSTR::new(),
            WBEM_FLAG_CONNECT_USE_MAX_WAIT.0,
            &BSTR::new(),
            None,
        )
    }
    .map_err(failure)?;
    secure(&services)?;
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let escaped = volume.guid_root.replace('\\', "\\\\").replace('\'', "\\'");
    let query = BSTR::from(format!(
        "SELECT ID, ProviderID, InstallDate, ClientAccessible, Persistent, VolumeName, State FROM Win32_ShadowCopy WHERE VolumeName = '{escaped}' AND State = 9"
    ));
    let enumerator = unsafe {
        services.ExecQuery(
            &BSTR::from("WQL"),
            &query,
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )
    }
    .map_err(failure)?;
    secure(&enumerator)?;
    let started = Instant::now();
    let mut copies: Vec<ShadowCopy> = Vec::with_capacity(COPY_LIMIT);
    let coverage = loop {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if started.elapsed() >= Duration::from_millis(ENUMERATION_WINDOW_MS) {
            break EnumerationCoverage::TimeLimit;
        }
        let mut objects = [None];
        let mut returned = 0;
        let status = unsafe { enumerator.Next(100, &mut objects, &mut returned) };
        status.ok().map_err(failure)?;
        if returned > 1 {
            return Err(Unavailable::InvalidMetadata);
        }
        if returned == 1 {
            let object = objects[0].as_ref().ok_or(Unavailable::InvalidMetadata)?;
            let entry = copy(object, volume)?;
            if copies.iter().any(|value| value.id == entry.id) {
                return Err(Unavailable::InvalidMetadata);
            }
            if copies.len() == COPY_LIMIT {
                break EnumerationCoverage::ItemLimit;
            }
            copies.push(entry);
        }
        if status.0 == WBEM_S_FALSE.0 {
            break EnumerationCoverage::Complete;
        }
        if status.0 != WBEM_S_NO_ERROR.0 && status.0 != WBEM_S_TIMEDOUT.0
            || returned == 0 && status.0 == WBEM_S_NO_ERROR.0
        {
            return Err(Unavailable::InvalidMetadata);
        }
    };
    copies.sort_by(|a, b| b.installed_at.cmp(&a.installed_at).then(a.id.cmp(&b.id)));
    Ok(Some(Discovery::Available { copies, coverage }))
}
